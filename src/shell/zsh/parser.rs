use std::collections::HashMap;
use std::ptr::NonNull;
use std::cell::{RefCell};
use bstr::{BStr, BString, ByteSlice, ByteVec};
use std::os::raw::{c_char, c_int};
use serde::{Deserialize};
use std::ops::Range;
use std::ptr::null_mut;
use super::bindings::{token, lextok, CommandStack};
use super::{MetaStr, MetaString};

fn untokenize(c: u8) -> u8 {
    unsafe {
        // ztokens has the wrong length, so use pointer arithmetic instead
        // search for the token bc sometimes it is len > 1
        #[allow(static_mut_refs)]
        let ztokens = zsh_sys::ztokens.as_ptr();
        *ztokens.add((c - token::Pound as u8) as usize) as u8
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(default)]
pub struct ParserOptions {
    pub comments: Option<bool>,
    pub custom: bool,
    pub tokens: Option<bool>,
}

impl Default for ParserOptions {
    fn default() -> Self {
        Self {
            comments: None,
            custom: true,
            tokens: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum TokenKind {
    #[default]
    None,
    Lextok(lextok),
    Token(token),
    CommandStack(CommandStack),
    Heredoc(bool),
    Initial,
    SyntaxError,
    Redirect,
    Comment,
    Command,
    Arg0,
    Scope(CommandStack),
}

impl TokenKind {

    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    fn from_token(val: u8) -> Self {
        num::FromPrimitive::from_u8(val).map_or(Self::None, Self::Token)
    }

    fn from_lextok(val: u32) -> Self {
        num::FromPrimitive::from_u32(val).map_or(Self::None, Self::Lextok)
    }

    fn from_command_stack(val: u8) -> Self {
        num::FromPrimitive::from_u8(val).map_or(Self::None, Self::CommandStack)
    }

    fn can_start_command(self) -> bool {
        match self {
            TokenKind::Lextok(lextok::THEN) => false,
            TokenKind::Lextok(lextok::DOLOOP) => false,
            TokenKind::Lextok(lextok::ELIF) => false,
            TokenKind::Lextok(lextok::ELSE) => false,
            TokenKind::Lextok(lextok::FI) => false,
            TokenKind::Lextok(lextok::DONE) => false,
            TokenKind::Lextok(lextok::ESAC) => false,
            _ => true,
        }
    }

    fn followed_by_command(self) -> bool {
        match self {
            TokenKind::Lextok(lextok::INPAR) => true,
            TokenKind::Lextok(lextok::INBRACE) => true,
            TokenKind::Lextok(lextok::IF) => true,
            TokenKind::Lextok(lextok::ELIF) => true,
            TokenKind::Lextok(lextok::WHILE) => true,
            TokenKind::Lextok(lextok::UNTIL) => true,
            TokenKind::Lextok(lextok::DOLOOP) => true,
            TokenKind::Lextok(lextok::THEN) => true,
            TokenKind::Lextok(lextok::ELSE) => true,
            TokenKind::Lextok(lextok::SEPER) => true,
            TokenKind::Lextok(lextok::AMPER) => true,
            TokenKind::Lextok(lextok::DAMPER) => true,
            TokenKind::Lextok(lextok::DBAR) => true,
            TokenKind::Lextok(lextok::BAR) => true,
            TokenKind::Initial => true,
            _ => false,
        }
    }

    fn ends_command(self) -> bool {
        match self {
            TokenKind::Lextok(lextok::OUTPAR) => true,
            TokenKind::Lextok(lextok::OUTBRACE) => true,
            TokenKind::Lextok(lextok::SEPER) => true,
            TokenKind::Lextok(lextok::AMPER) => true,
            TokenKind::Lextok(lextok::DAMPER) => true,
            TokenKind::Lextok(lextok::DBAR) => true,
            TokenKind::Lextok(lextok::BAR) => true,
            TokenKind::Initial => true,
            TokenKind::CommandStack(CommandStack::Cmdor) => true,
            TokenKind::CommandStack(CommandStack::Cmdand) => true,
            TokenKind::CommandStack(CommandStack::Pipe) => true,
            _ => false,
        }
    }

}

impl std::fmt::Display for TokenKind {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            TokenKind::None => write!(fmt, ""),
            TokenKind::Lextok(k) => write!(fmt, "{k:?}"),
            TokenKind::Token(k) => write!(fmt, "{k:?}"),
            TokenKind::CommandStack(k) => write!(fmt, "{k:?}"),
            TokenKind::Redirect => write!(fmt, "redirect"),
            TokenKind::Comment => write!(fmt, "comment"),
            TokenKind::Command => write!(fmt, "command"),
            TokenKind::Initial => write!(fmt, "initial"),
            TokenKind::Heredoc(_quoted) => write!(fmt, "heredoc"),
            TokenKind::Arg0 => write!(fmt, "arg0"),
            TokenKind::Scope(_) => write!(fmt, "scope"),
            TokenKind::SyntaxError => write!(fmt, "syntax_error"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub range: Range<usize>,
    pub kind: TokenKind,
    pub children: Option<Vec<Token>>,
}

impl Token {

    fn new(range: Range<usize>) -> Self {
        Self::new_with_kind(range, TokenKind::Initial)
    }

    fn new_with_kind(range: Range<usize>, kind: TokenKind) -> Self {
        Self {
            range,
            kind,
            children: None,
        }
    }

    fn as_str<'a>(&self, cmd: &'a BStr) -> &'a BStr {
        &cmd[self.range.clone()]
    }

    #[allow(dead_code)]
    fn debug_dump(&self, cmd: &BStr, indent: usize) -> String {
        let mut string = format!(
            "{:indent$}Token{{ range: {:?}, kind: {:?}, str: {:?} }}",
            "",
            self.range,
            self.kind,
            if self.range.start > self.range.end {
                None
            } else {
                Some(self.as_str(cmd))
            },
        );
        for token in self.children.iter().flatten() {
            string.push('\n');
            string.push_str(&token.debug_dump(cmd, indent + 2));
        }
        string
    }

    fn children_end(&self) -> Option<usize> {
        self.children.as_ref().and_then(|n| n.last()).map(|t| t.range.end)
    }

    fn get_children_mut(&mut self) -> &mut Vec<Token> {
        self.children.get_or_insert_default()
    }

    fn push_token(&mut self, token: Token) -> &mut Token {
        self.get_children_mut().push_mut(token)
    }

    fn unmeta_range(&mut self, cmd: &MetaStr, cache: &mut [usize; 256]) {
        // convert ranges from meta to unmeta
        for val in [&mut self.range.start, &mut self.range.end] {
            if *val > 0 {
                match cache.get(*val) {
                    Some(0) => { // not yet cached
                        let x = cmd.len_up_to(*val);
                        cache[*val] = x;
                        *val = x;
                    },
                    Some(&x) => { // cached
                        *val = x;
                    },
                    None => { // out of cache range
                        *val = cmd.len_up_to(*val);
                    },
                }
            }
        }
    }

    fn apply_custom_token(&mut self) {
        let Some(children) = self.children.as_mut()
            else { return };

        let mut i = 0;
        while i < children.len() {
            let slice = &children[i..];
            let action = match slice {
                // (<|>|>>|<>|>\||>!|<&|>&|>&\|>&!|&>\||&>!|>>&|&>>|>>&\||>>&!|&>>\||&>>!) STRING
                [
                    Token{kind: TokenKind::Lextok(lextok::OUTANG | lextok::OUTANGBANG | lextok::DOUTANG | lextok::DOUTANGBANG | lextok::INANG | lextok::INOUTANG | lextok::INANGAMP | lextok::OUTANGAMP | lextok::AMPOUTANG | lextok::OUTANGAMPBANG | lextok::DOUTANGAMP | lextok::DOUTANGAMPBANG | lextok::TRINANG), ..},
                    // Token{kind: TokenKind::None | TokenKind::Lextok(lextok::STRING | lextok::LEXERR), ..},
                ..] => Some((TokenKind::Redirect, 2)),

                _ => None,
            };

            if let Some((kind, len)) = action && i+len <= children.len() {
                let start = children[i].range.start;
                let end = children[i+len-1].range.end;
                let token = Token::new_with_kind(start..end, kind);
                let nested = children.splice(i..i+len, [token]).collect();
                children[i].children = Some(nested);
            }

            i += 1;
        }
    }

    fn postprocess(
        &mut self,
        string: &BStr,
        meta: &MetaStr,
        meta_cache: &mut [usize; 256],
        allow_comments: bool,
    ) {
        // lstrip
        let nonblank = meta.to_bytes()[self.range.clone()]
            .iter()
            .position(|c| !super::zistype(*c, zsh_sys::IBLANK));
        self.range.start = nonblank.map_or(self.range.end, |x| self.range.start + x);

        // convert ranges from meta to unmeta
        self.unmeta_range(meta, meta_cache);

        // remove empty children
        if let Some(children) = &mut self.children {
            children.retain_mut(|token| {
                token.postprocess(string, meta, meta_cache, allow_comments);
                !token.range.is_empty()
            });

            if children.is_empty() {
                self.children = None;
            }
        }

        // detect comments parsed as a single string item
        if allow_comments
            && self.children.is_none()
            && matches!(self.kind, TokenKind::Lextok(lextok::STRING))
            && self.as_str(string).starts_with(b"#")
        {
            self.kind = TokenKind::Comment;
        }

        self.apply_custom_token();

        if matches!(self.kind, TokenKind::Command) && let Some(children) = &mut self.children {
            // collapse empty command with only a command ending thing or comment
            if children.len() == 1
                && (children[0].kind.ends_command() || matches!(children[0].kind, TokenKind::Comment))
            {
                *self = children.pop().unwrap();
            } else {
                // first string is the command
                for t in children.iter_mut() {
                    if matches!(t.kind, TokenKind::None | TokenKind::Scope(_) | TokenKind::Lextok(lextok::STRING)) {
                        t.kind = TokenKind::Arg0;
                        break
                    }
                }
            }
        }

        // clamp end to children end
        if let Some(end) = self.children_end() {
            self.range.end = end;
        }
    }

    fn add_tokstr(&mut self, offset: usize, tokstr: &[u8]) {
        let children = self.get_children_mut();

        // add some tokens
        for (i, &c) in tokstr.into_iter().enumerate() {
            let range = offset + i .. offset + i + 1;
            if let Some(tok) = children.last_mut() && tok.range.end > range.start {
                // do nothing if overlapping with previous
            } else if super::is_token(c) {
                children.push(Token::new_with_kind(range, TokenKind::from_token(c)));
            } else if let Some(prev @ Token{kind: TokenKind::None, ..}) = children.last_mut() {
                prev.range.end = range.end;
            } else {
                children.push(Token::new_with_kind(range, TokenKind::None));
            }
        }
    }


}

pub fn parse(mut cmd: BString, options: ParserOptions) -> (bool, Vec<Token>) {
    if cmd.trim().is_empty() {
        return (true, vec![])
    }
    // add newline at the end
    cmd.push_str(b"\n");
    parse_internal(cmd, options)
}

#[derive(Default)]
struct ParseState {
    meta: MetaString,
    metalen: usize,
    start: usize,
    stack: Vec<Token>,
    tokstr_map: HashMap<Option<NonNull<c_char>>, usize>,
    cmdsp: i32,
    started: bool,
    tokstr: *const c_char,
}

thread_local! {
    static PARSE_STATE: RefCell<ParseState> = RefCell::new(ParseState::default());
}

impl ParseState {

    fn reset(&mut self, meta: MetaString) {
        self.meta = meta;
        self.metalen = self.meta.count_bytes();
        self.start = 0;
        self.stack.clear();
        self.stack.push(Token::new(0..self.metalen));
        self.stack.push(Token::new_with_kind(0..0, TokenKind::Command));
        self.tokstr_map.clear();

        self.cmdsp = 0;
        self.started = false;
    }

    fn pop(&mut self, end: Option<usize>,) -> &mut Token {
        self.pop_with_meta(end).0
    }

    fn pop_with_meta(&mut self, end: Option<usize>,) -> (&mut Token, &MetaStr) {
        let mut token = self.stack.pop().unwrap();
        if let Some(end) = end {
            token.range.end = token.range.end.min(end);
        }

        if self.stack.is_empty() {
            // wtf no parent tokens?
            (self.stack.push_mut(token), self.meta.as_ref())
        } else {
            (self.stack.last_mut().unwrap().push_token(token), self.meta.as_ref())
        }
    }

    fn get_current_index(&self) -> usize {
        unsafe {
            self.metalen.saturating_sub(zsh_sys::inbufct as usize)
        }
    }

    fn parse_heredoc(meta: &MetaStr, range: Range<usize>) -> Option<(usize, Vec<Token>)> {
        let heredoc = &meta.to_bytes()[range.clone()];

        // get second last newline
        let end = heredoc.iter()
            .enumerate()
            .rev()
            .filter_map(|(i, &c)| (c == b'\n').then_some(i))
            .nth(1)?;
        let heredoc = &heredoc[..end];
        let marker_len = range.len() - end - 1;

        let string = std::ffi::CString::new(heredoc).unwrap();
        let mut string_ptr = string.as_ptr().cast_mut();

        // parse it
        let tokstr = unsafe {
            let err = zsh_sys::parsestrnoerr(&raw mut string_ptr);
            if err != 0 {
                return None;
            }

            // apparently it can be modified?
            std::ffi::CStr::from_ptr(string_ptr)
        };

        let mut token = Token::new(range.start .. range.start + string.count_bytes());
        token.add_tokstr(range.start, tokstr.to_bytes());
        token.push_token(Token::new_with_kind(range.end - 1 - marker_len .. range.end - marker_len, TokenKind::None));

        Some((range.start + end + 1, token.children.unwrap()))
    }

    fn push_command_stack(&mut self) {
        let end = self.get_current_index();

        unsafe {
            // pop previous if end of command
            let cs = *zsh_sys::cmdstack.add(self.cmdsp as usize);
            let kind = TokenKind::from_command_stack(cs);
            if kind.ends_command() {
                self.pop(Some(end));
            }

            // add tokstr from before this command stack
            let scope = num::FromPrimitive::from_u8(cs).map_or(TokenKind::None, TokenKind::Scope);
            self.stack.push(Token::new_with_kind(end .. end, scope));
            self.add_tokstr();
            let token = self.stack.last_mut().unwrap();
            token.range.start = token.children.as_ref().and_then(|c| c.first()).map_or(end, |c| c.range.start);

            // token for the actual command stack
            let mut command = Token::new_with_kind(end .. self.metalen, kind);
            let mut initial = Token::new(end..end);

            // check if heredoc is quoted
            if matches!(kind, TokenKind::CommandStack(CommandStack::Heredoc))
                && let Some(hdocs) = zsh_sys::hdocs.as_ref()
                && !hdocs.str_.is_null()
            {
                let word = MetaStr::from_ptr(hdocs.str_);
                let quoted = word.to_bytes().iter().any(|&c| super::zistype(c, zsh_sys::INULL));
                command.kind = TokenKind::Heredoc(quoted);
                initial.kind = TokenKind::None;
            }

            self.start = command.range.start;
            command.push_token(initial);
            self.stack.push(command);
        }
    }

    fn pop_command_stack(&mut self) {
        let start = self.start;
        let end = self.get_current_index();

        let (token, meta) = loop {
            let (token, meta) = self.pop_with_meta(Some(end));
            // clamp to children - need to do this or add_tokstr() won't work
            if let Some(end) = token.children_end() {
                token.range.end = end;
            }
            if matches!(token.kind, TokenKind::Heredoc(_) | TokenKind::CommandStack(_)) {
                break (token, meta);
            }
        };

        if let TokenKind::Heredoc(quoted) = token.kind {
            // heredocs dont parse well
            // this is because zsh pushes a new context that overrides our getc
            // so we try reparse it ourselves
            if !quoted && let Some((marker, mut heredoc)) = Self::parse_heredoc(meta, start..end) {
                token.get_children_mut().append(&mut heredoc);
                let tokens = self.stack.last_mut().unwrap().get_children_mut();
                // marker
                tokens.push(Token::new(marker .. end-1));
                // newline
                tokens.push(Token::new_with_kind(end-1 .. end, TokenKind::Lextok(lextok::NEWLIN)));
            } else {
                token.push_token(Token::new_with_kind(start .. end, TokenKind::None));
            }

        } else if matches!(token.kind, TokenKind::CommandStack(CommandStack::Quote | CommandStack::Dquote))
            && !token.range.is_empty()
        {
            // fill in missing bits of the string except the quotes
            token.range.end = end - 1;
        }

        // // fill in missing bits around the command stack
        self.add_tokstr();
        self.pop(Some(end));

        self.start = end;
    }

    fn add_tokstr(&mut self) {
        let end = self.get_current_index();
        unsafe {
            let ptr = NonNull::new(self.tokstr.cast_mut()).or(NonNull::new(zsh_sys::tokstr));
            let tokstr = ptr.map_or(meta_str!(c""), |ptr| MetaStr::from_ptr(ptr.as_ptr()));
            let consumed = self.tokstr_map.get(&ptr).copied().unwrap_or(0);
            self.tokstr_map.insert(ptr, tokstr.count_bytes());

            let tokstr = &tokstr.to_bytes()[consumed..];

            // a lot of the time tokstr ends at end, but sometimes it is before
            let untokenized = tokstr.iter().copied().map(untokenize);
            let start = (self.start .. end - tokstr.len())
                .find(|&i| self.meta.to_bytes()[i .. i + tokstr.len()].iter().copied().eq(untokenized.clone()));
            let start = start.unwrap_or(end - tokstr.len());
            self.start = start + tokstr.len();

            self.stack.last_mut().unwrap().add_tokstr(start, tokstr);
        }
    }

    fn getc(&mut self) -> c_int {
        unsafe {
            if self.started {

                let start = self.start;
                let end = self.get_current_index();

                if zsh_sys::cmdsp > self.cmdsp {
                    self.push_command_stack();
                } else if zsh_sys::cmdsp < self.cmdsp {
                    self.pop_command_stack();
                }

                // handle tokstr
                self.add_tokstr();

                // normal token
                if start != end && zsh_sys::tok != zsh_sys::lextok_ENDINPUT {
                    let token = Token::new_with_kind(start .. end, TokenKind::from_lextok(zsh_sys::tok));

                    let prev = self.stack.last().unwrap();
                    let prev = prev.children.as_ref().and_then(|c| c.last()).unwrap_or(prev);

                    // this token starts a command
                    if prev.kind.followed_by_command() && token.kind.can_start_command() {
                        self.stack.push(Token::new_with_kind(start..self.metalen, TokenKind::Command));
                    }

                    // this token ends a command
                    if token.kind.ends_command() && matches!(self.stack.last().unwrap().kind, TokenKind::Command) {
                        self.pop(Some(start));
                    }

                    self.stack.last_mut().unwrap().push_token(token);
                    self.start = end;
                }

                self.cmdsp = zsh_sys::cmdsp;

            } else {
                self.started = true;
            }

            zsh_sys::tok = zsh_sys::lextok_ENDINPUT;
            self.tokstr = zsh_sys::tokstr;

            zsh_sys::ingetc()
        }
    }

    fn ungetc(&mut self, c: c_int) {
        unsafe {
            zsh_sys::inungetc(c);
        }
    }
}

unsafe extern "C" fn hgetc_override() -> c_int {
    PARSE_STATE.with_borrow_mut(|state| state.getc())
}

unsafe extern "C" fn hungetc_override(c: c_int) {
    PARSE_STATE.with_borrow_mut(|state| state.ungetc(c));
}

fn parse_internal(
    cmd: BString,
    options: ParserOptions,
) -> (bool, Vec<Token>) {

    let metafied = MetaString::from(cmd.clone());
    let metalen = metafied.count_bytes();
    let mut complete = true;

    unsafe {
        zsh_sys::zcontext_save();
        zsh_sys::inpush(metafied.as_ptr().cast_mut(), 0, null_mut());
        zsh_sys::strinbeg(0);
        let old_noerrs = super::set_error_verbosity(super::ErrorVerbosity::Ignore);
        let old_noaliases = zsh_sys::noaliases;
        zsh_sys::noaliases = 1;
        zsh_sys::incmdpos = 1;
        zsh_sys::errflag = 0;

        let old_lexflags = zsh_sys::lexflags;
        let mut new_lexflags = zsh_sys::LEXFLAGS_ACTIVE | zsh_sys::LEXFLAGS_ZLE;
        let allow_comments = options.comments.unwrap_or(super::isset(zsh_sys::INTERACTIVECOMMENTS as _));
        if allow_comments {
            new_lexflags |= zsh_sys::LEXFLAGS_COMMENTS_KEEP;
        }
        zsh_sys::lexflags = new_lexflags as _;

        let need_tokens = options.tokens.unwrap_or(true);

        let old_hgetc = zsh_sys::hgetc;
        let old_hungetc = zsh_sys::hungetc;
        if need_tokens {
            zsh_sys::hgetc = Some(hgetc_override);
            zsh_sys::hungetc = Some(hungetc_override);
        }

        if need_tokens {
            PARSE_STATE.with_borrow_mut(|state| {
                state.reset(metafied);
            });
        }

        // we are complete if all the pointers are valid
        // we dont need to free, zsh uses zhalloc
        while zsh_sys::lexstop == 0 && zsh_sys::inbufct > 0 {
            complete = !zsh_sys::parse_event(zsh_sys::lextok_ENDINPUT as _).is_null() && complete;
        }

        // merge everything together and postprocess
        let tokens = if need_tokens {
            PARSE_STATE.with_borrow_mut(|state| {
                while state.stack.len() > 1 {
                    state.pop(None);
                }
                let mut token = state.stack.pop().unwrap();
                token.postprocess(cmd.as_ref(), state.meta.as_ref(), &mut [0; _], allow_comments);
                let end = token.children_end().unwrap_or(0);
                if !complete && end < metalen {
                    token.push_token(Token::new_with_kind(end .. metalen, TokenKind::SyntaxError));
                }
                // ::log::debug!("DEBUG(curved)\t{}\t=\n{}", stringify!(s.debug_dump(cmd.as_ref(), 0)), token.debug_dump(cmd.as_ref(), 0));

                token.children
            })
        } else {
            None
        };

        // restore
        zsh_sys::hgetc = old_hgetc;
        zsh_sys::hungetc = old_hungetc;
        zsh_sys::lexflags = old_lexflags;
        zsh_sys::strinend();
        zsh_sys::inpop();
        zsh_sys::errflag &= !zsh_sys::errflag_bits_ERRFLAG_ERROR as i32;
        zsh_sys::noaliases = old_noaliases;
        super::set_error_verbosity(old_noerrs);
        zsh_sys::zcontext_restore();

        (complete, tokens.unwrap_or_else(Vec::new))
    }
}
