use std::ptr::NonNull;
use std::cell::{RefCell};
use bstr::{BStr, BString, ByteSlice, ByteVec};
use std::os::raw::{c_char, c_int};
use serde::{Deserialize};
use std::ops::Range;
use std::ptr::null_mut;
use super::bindings::{token, lextok, CommandStack};
use super::{MetaStr, MetaString};

#[derive(Clone, Copy, Deserialize)]
#[serde(default)]
pub struct ParserOptions {
    comments: Option<bool>,
    custom: bool,
}

impl Default for ParserOptions {
    fn default() -> Self {
        Self {
            comments: None,
            custom: true,
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
            TokenKind::SyntaxError => write!(fmt, "stynax_error"),
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

    const fn new(range: Range<usize>) -> Self {
        Self::new_with_kind(range, TokenKind::Initial)
    }

    const fn new_with_kind(range: Range<usize>, kind: TokenKind) -> Self {
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
                let end = children[i+len].range.end;
                let token = Token::new_with_kind(start..end, kind);
                let nested = children.splice(i..i+len, [token]).collect();
                children[i].children = Some(nested);
            }

            i += 1;
        }
    }

    fn postprocess(&mut self, string: &BStr, meta: &MetaStr, meta_cache: &mut [usize; 256], allow_comments: bool) {
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

        // collapse empty command with only a command ending thing or comment
        if matches!(self.kind, TokenKind::Command)
            && let Some(children) = &mut self.children
            && children.len() == 1
            && (children[0].kind.ends_command() || matches!(children[0].kind, TokenKind::Comment))
        {
            *self = children.pop().unwrap();
        }

        // clamp end to children end
        if let Some(end) = self.children_end() {
            self.range.end = end;
        }
    }

    fn add_tokstr(&mut self, tokstr: &[u8]) {
        // process the tokstr
        let range = self.range.clone();
        let children = self.get_children_mut();

        let mut tokstr = tokstr.iter()
            .enumerate()
            .filter(|(_, c)| super::is_token(**c))
            .map(|(i, &c)| (range.start + i, c))
            .peekable();

        // this intersperses children and tokens and simple strings
        let mut start = range.start;
        let mut childi = 0;
        loop {
            let next_child = children.get(childi);
            let next_token = tokstr.peek();

            if let Some(child) = next_child && next_token.is_none_or(|(i, _)| child.range.start <= *i) {
                // child comes sooner
                let range = child.range.clone();
                children.insert(childi, Token::new_with_kind(start..range.start, TokenKind::None));
                childi += 2;
                start = range.end;
            } else if let Some((i, _)) = next_token && next_child.is_none_or(|c| *i < c.range.start) {
                let (i, tok) = tokstr.next().unwrap();
                if childi > 0 {
                    // in case the child overlaps with a token
                    start = children[childi-1].range.end.min(i);
                    children[childi-1].range.end = start;
                }
                children.insert(childi, Token::new_with_kind(start..i, TokenKind::None));
                children.insert(childi+1, Token::new_with_kind(i..i+1, TokenKind::from_token(tok)));
                childi += 2;
                start = i+1;
            } else {
                // no more
                children.push(Token::new_with_kind(start..range.end, TokenKind::None));
                break
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

    let (mut complete, token) = parse_internal(cmd, options);
    if token.children.as_ref().is_none_or(|c| c.is_empty()) {
        // no tokens???
        complete = false;
    }

    (complete, token.children.unwrap())
}

#[derive(Default)]
struct ParseState {
    meta: MetaString,
    metalen: usize,
    start: usize,
    stack: Vec<Token>,
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
        self.stack.push(Token::new(0..0));

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

        // TODO can i use add_tokstr() here?
        let mut tokens = vec![];
        let mut start = 0;
        for (i, &c) in tokstr.to_bytes().iter().enumerate() {
            if super::is_token(c) {
                tokens.push(Token::new_with_kind(range.start + start .. range.start + i, TokenKind::None));
                let kind = TokenKind::from_token(c);
                tokens.push(Token::new_with_kind(range.start + i .. range.start + i + 1, kind));
                start = i + 1;
            }
        }
        tokens.push(Token::new_with_kind(range.start + start .. range.start + end, TokenKind::None));
        tokens.push(Token::new_with_kind(range.start + end .. range.start + end + 1, TokenKind::Lextok(lextok::NEWLIN)));

        Some((range.start + end + 1, tokens))
    }

    fn getc(&mut self) -> c_int {
        unsafe {
            if self.started {

                let start = self.start;
                let end = self.get_current_index();

                if zsh_sys::cmdsp > self.cmdsp {
                    // push stack

                    // add tokstr from before this command stack
                    let tokstr = NonNull::new(self.tokstr.cast_mut()).or(NonNull::new(zsh_sys::tokstr));
                    let tokstr_len = tokstr.map_or(0, |ptr| MetaStr::from_ptr(ptr.as_ptr()).count_bytes());
                    self.stack.push(Token::new_with_kind(end - tokstr_len .. self.metalen, TokenKind::None));

                    let cs = *zsh_sys::cmdstack.add(self.cmdsp as usize);
                    let kind = TokenKind::from_command_stack(cs);
                    if kind.ends_command() {
                        self.pop(Some(end));
                    }

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
                    // new command stack and its initial token
                    command.push_token(initial);
                    self.stack.push(command);

                } else if zsh_sys::cmdsp < self.cmdsp {
                    // pop stack

                    let (token, meta) = loop {
                        let (token, meta) = self.pop_with_meta(Some(end));
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

                    } else if matches!(token.kind, TokenKind::CommandStack(CommandStack::Quote | CommandStack::Dquote)) && !token.range.is_empty() {
                        // fill in missing bits of the string
                        token.add_tokstr(b"");
                    }

                    // fill in missing bits around the command stack
                    let token = self.pop(Some(end));
                    if !zsh_sys::tokstr.is_null() {
                        let tokstr = MetaStr::from_ptr(zsh_sys::tokstr).to_bytes();
                        token.add_tokstr(tokstr);
                    }

                    self.start = end;
                }

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
) -> (bool, Token) {

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

        let old_hgetc = zsh_sys::hgetc;
        zsh_sys::hgetc = Some(hgetc_override);
        let old_hungetc = zsh_sys::hungetc;
        zsh_sys::hungetc = Some(hungetc_override);

        PARSE_STATE.with_borrow_mut(|state| {
            state.reset(metafied);
        });

        // we are complete if all the pointers are valid
        // we dont need to free, zsh uses zhalloc
        while zsh_sys::lexstop == 0 && zsh_sys::inbufct > 0 {
            complete = !zsh_sys::parse_event(zsh_sys::lextok_ENDINPUT as _).is_null() && complete;
        }

        // merge everything together and postprocess
        let token = PARSE_STATE.with_borrow_mut(|state| {
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

            token
        });

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

        (complete, token)
    }
}
