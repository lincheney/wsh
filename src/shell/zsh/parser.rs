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
    Substitution,
    Redirect,
    Function,
    Comment,
    Command,
    HeredocOpenTag,
    HeredocCloseTag,
    HeredocBody,
}

impl TokenKind {

    pub fn is_none(&self) -> bool {
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

    fn can_start_command(&self) -> bool {
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

    fn followed_by_command(&self) -> bool {
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

    fn ends_command(&self) -> bool {
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
            TokenKind::Substitution => write!(fmt, "substitution"),
            TokenKind::Redirect => write!(fmt, "redirect"),
            TokenKind::Function => write!(fmt, "function"),
            TokenKind::HeredocOpenTag => write!(fmt, "heredoc_open_tag"),
            TokenKind::HeredocCloseTag => write!(fmt, "heredoc_close_tag"),
            TokenKind::HeredocBody => write!(fmt, "heredoc_body"),
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

    pub fn as_str<'a>(&self, cmd: &'a BStr, range_offset: usize) -> &'a BStr {
        &cmd[self.range.start - range_offset .. self.range.end - range_offset]
    }

    pub fn remove_dummy_from_children(&mut self, len: usize) {
        let mut tok = self;
        while let Some(children) = tok.children.as_mut() && let Some(last) = children.last_mut() {
            last.range.end -= len;
            if last.range.is_empty() {
                children.pop();
                break
            }
            tok = children.last_mut().unwrap();
        }
    }

    fn debug_dump(&self, cmd: &BStr, indent: usize) -> String {
        let mut string = format!(
            "{:indent$}Token{{ range: {:?}, kind: {:?}, str: {:?} }}",
            "",
            self.range,
            self.kind,
            if self.range.start > self.range.end {
                None
            } else {
                Some(self.as_str(cmd, 0))
            },
        );
        for token in self.children.iter().flatten() {
            string.push_str("\n");
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

    fn postprocess(&mut self, meta: &MetaStr, meta_cache: &mut [usize; 256]) {
        // lstrip
        let nonblank = meta.to_bytes()[self.range.clone()]
            .iter()
            .position(|c| !super::zistype(*c, zsh_sys::IBLANK));
        self.range.start = nonblank.map_or(self.range.end, |x| self.range.start + x);

        // convert ranges from meta
        self.unmeta_range(meta, meta_cache);

        // remove empty children
        if let Some(children) = &mut self.children {
            children.retain_mut(|token| {
                token.postprocess(meta, meta_cache);
                !token.range.is_empty()
            });

            if children.is_empty() {
                self.children = None;
            }
        }

        // add missing strings
        if !self.range.is_empty()
            && matches!(self.kind, TokenKind::CommandStack(CommandStack::Dquote | CommandStack::Quote))
        {
            let Range{start, mut end} = self.range;
            let children = self.get_children_mut();
            for i in (0..children.len()).rev() {
                let start = children[i].range.end;
                if start != end {
                    children.insert(i+1, Self::new_with_kind(start..end, TokenKind::None))
                }
                end = children[i].range.start;
            }
            if start != end {
                children.insert(0, Self::new_with_kind(start..end, TokenKind::None))
            }
        }

        // empty command with only a command ending thing
        if matches!(self.kind, TokenKind::Command)
            && let Some(children) = &mut self.children
            && children.len() == 1 && children[0].kind.ends_command()
        {
            *self = children.pop().unwrap();
        }

        if let Some(end) = self.children_end() {
            self.range.end = end;
        }
    }

    fn detect_comments(&mut self, string: &BStr) {
        if let Some(children) = &mut self.children {
            for child in children {
                child.detect_comments(string);
            }
        } else if matches!(self.kind, TokenKind::Lextok(lextok::STRING))
            && self.as_str(string, 0).starts_with(b"#")
        {
            self.kind = TokenKind::Comment;
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
    tokstr_len: usize,
    started: bool,
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
        self.tokstr_len = 0;
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
            .skip(1)
            .next()?;
        let heredoc = &heredoc[..end];

        let string = std::ffi::CString::new(heredoc).unwrap();
        let mut string_ptr = string.as_ptr().cast_mut();

        let tokstr = unsafe {
            let err = zsh_sys::parsestrnoerr(&raw mut string_ptr);
            if err != 0 {
                return None;
            }

            // apparently it can be modified?
            std::ffi::CStr::from_ptr(string_ptr)
        };

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

                let mut tokstr_kind = None;

                let tokstr = if zsh_sys::tokstr.is_null() {
                    meta_str!(c"")
                } else {
                    MetaStr::from_ptr(zsh_sys::tokstr)
                };
                // check if last char is a special token
                if tokstr.count_bytes() != self.tokstr_len
                    && let Some(c) = tokstr.last()
                    && super::is_token(c)
                {
                    tokstr_kind = Some((c, TokenKind::from_token(c)));
                }
                self.tokstr_len = tokstr.count_bytes();

                if zsh_sys::cmdsp > self.cmdsp {
                    // push stack
                    // finish current token
                    let cs = *zsh_sys::cmdstack.add(self.cmdsp as usize);
                    let kind = TokenKind::from_command_stack(cs);
                    self.pop(Some(start));
                    if kind.ends_command() {
                        self.pop(Some(start));
                    }
                    ::log::debug!("DEBUG(sewer) \t{}\t= {:?}", stringify!(token.kind), kind);

                    let mut command = Token::new_with_kind(start .. self.metalen, kind);
                    let mut initial = Token::new(start..start);

                    if matches!(kind, TokenKind::CommandStack(CommandStack::Heredoc))
                        && let Some(hdocs) = zsh_sys::hdocs.as_ref()
                        && !hdocs.str_.is_null()
                    {
                        let word = MetaStr::from_ptr(hdocs.str_);
                        let quoted = word.to_bytes().iter().any(|&c| super::zistype(c, zsh_sys::INULL));
                        command.kind = TokenKind::Heredoc(quoted);
                        initial.kind = TokenKind::None;
                    }

                    // new command stack
                    self.stack.push(command);
                    // and its initial token
                    self.stack.push(initial);

                } else if zsh_sys::cmdsp < self.cmdsp {
                    // pop stack
                    self.pop(Some(end));
                    let (token, meta) = self.pop_with_meta(Some(end));
                    ::log::debug!("DEBUG(flames)\t{}\t= {:?}", stringify!("pop"), ("pop", token.kind));

                    if matches!(
                        (token.kind, tokstr_kind),
                        (TokenKind::CommandStack(CommandStack::Dquote), Some((_, TokenKind::Token(token::Dnull))))
                        | (TokenKind::CommandStack(CommandStack::Quote), Some((_, TokenKind::Token(token::Snull))))
                    ) {
                        // insert the quote here
                        let t = Token::new_with_kind(end-1 .. end, tokstr_kind.take().unwrap().1);
                        token.push_token(t);
                    }

                    if let TokenKind::Heredoc(quoted) = token.kind {

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
                    }

                    self.start = end;
                }

                if let Some((c, kind)) = tokstr_kind {
                    // added a token
                    ::log::debug!("DEBUG(which) \t{}\t= {:?}", stringify!(kind), kind);

                    // ztokens has the wrong length, so use pointer arithmetic instead
                    // search for the token bc sometimes it is len > 1
                    #[allow(static_mut_refs)]
                    let ztokens = zsh_sys::ztokens.as_ptr();
                    let c = *ztokens.add((c - token::Pound as u8) as usize) as u8;

                    let start = start + self.meta.to_bytes()[start..end]
                        .iter()
                        .rposition(|&b| b == c)
                        .unwrap_or(end-1);

                    self.pop(Some(start));
                    self.stack.push(Token::new_with_kind(start .. end, kind));
                    self.start = end;
                }

                if start != end && zsh_sys::tok != zsh_sys::lextok_ENDINPUT {
                    let token = Token {
                        range: start .. end,
                        kind: TokenKind::from_lextok(zsh_sys::tok),
                        children: None,
                    };
                    ::log::debug!("DEBUG(hanks) \t{}\t= {:?}", stringify!(token), token);

                    // new token
                    let prev = self.pop(Some(start));
                    if prev.kind.followed_by_command() && token.kind.can_start_command() {
                        self.stack.push(Token::new_with_kind(start..self.metalen, TokenKind::Command));
                    }
                    if self.stack.last().unwrap().kind.ends_command() {
                        self.pop(Some(start));
                    }
                    self.stack.push(token);
                    self.start = end;
                }

                self.cmdsp = zsh_sys::cmdsp;

            } else {
                self.started = true;
            }
            zsh_sys::tok = zsh_sys::lextok_ENDINPUT;

            let c = zsh_sys::ingetc();
            // self.string_index += 1;
            ::log::debug!("DEBUG(wryest)\t{}\t= {:?}", stringify!(c), char::from(c as u8));
            c

        }
    }

    fn ungetc(&mut self, c: c_int) {
        // ::log::debug!("DEBUG(deters)\t{}\t= {:?}", stringify!(c), c);
        unsafe {
            zsh_sys::inungetc(c);
        }
        let end = self.get_current_index();
        let token = self.stack.last_mut().unwrap();
        token.range.end = token.range.end.min(end);
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

    let metafied = MetaString::from(cmd.to_owned());
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
        while zsh_sys::lexstop == 0 && zsh_sys::inbufct > 0 {
            complete = !zsh_sys::parse_event(zsh_sys::lextok_ENDINPUT as _).is_null() && complete;
        }
        let token = PARSE_STATE.with_borrow_mut(|state| {
            while state.stack.len() > 1 {
                state.pop(None);
            }
            let mut token = state.stack.pop().unwrap();
            token.postprocess(state.meta.as_ref(), &mut [0; _]);
            if allow_comments {
                token.detect_comments(cmd.as_ref());
            }
            let end = token.children_end().unwrap_or(0);
            if !complete && end < metalen {
                token.push_token(Token::new_with_kind(end .. metalen, TokenKind::SyntaxError));
            }
            ::log::debug!("DEBUG(curved)\t{}\t=\n{}", stringify!(s.debug_dump(cmd.as_ref(), 0)), token.debug_dump(cmd.as_ref(), 0));

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

    // if let Some(dummy) = dummy && let Some(last) = tokens.last_mut() {
        // debug_assert_eq!(last.range.end, cmd.len());
//
        // // if the last token is just the dummy, pop it
        // if last.range.start == cmd.len() - 1 {
            // tokens.pop();
        // } else {
            // // otherwise it must be joined onto an incomplete token
            // if !matches!(last.kind, Some(TokenKind::Comment)) {
                // complete = false;
            // }
            // last.range.end = last.range.end.min(cmd.len() - dummy.len());
            // if last.range.is_empty() {
                // tokens.pop();
            // } else {
                // last.remove_dummy_from_children(dummy.len());
            // }
        // }
        // heredocs.retain(|(i, _)| *i < tokens.len());
        // cmd = &cmd[..cmd.len() - dummy.len()];
    // }
//
    // complete = find_heredocs(cmd, &mut tokens, range_offset, &heredocs) && complete;
    // if options.custom {
        // let len = tokens.len();
        // apply_custom_token(cmd, options, &mut tokens, 0, len, range_offset);
    // }

}

fn find_heredocs(cmd: &BStr, tokens: &mut Vec<Token>, range_offset: usize, heredocs: &Vec<(usize, *mut c_char)>) -> bool {

    let mut offset = 0;
    let mut prev_index = 0;

    for (index, tokstr) in heredocs {
        let index = index.saturating_sub(offset);
        if index < prev_index {
            // this heredoc has probably been deleted as hit
            continue
        }
        prev_index = index;

        let tag = unsafe { MetaStr::from_ptr(zsh_sys::quotesubst(*tokstr)) }.to_bytes();
        let allow_tabs = matches!(tokens[index-1].kind, TokenKind::Lextok(lextok::DINANGDASH));
        let allow_subst = !tokens[index].as_str(cmd, range_offset).iter().any(|&c| matches!(c, b'\''|b'"'|b'\\'));
        tokens[index].kind = TokenKind::HeredocOpenTag;

        let mut newlines = tokens[index+1..]
            .iter()
            .enumerate()
            .filter(|(_, t)| t.as_str(cmd, range_offset) == b"\n")
            .map(|(i, _)| index + 1 + i);

        // look for the first newline
        let Some(heredoc_start) = newlines.next()
            else { return false };
        let mut heredoc_end = None;

        // iterate over all lines
        let mut prev_newline = heredoc_start;
        for i in newlines.chain(std::iter::once(tokens.len())) {

            let start = tokens[prev_newline].range.end;
            let end = tokens.get(i).map_or(cmd.len(), |tok| tok.range.start);

            let line = &cmd[start .. end];
            if line == tag || (allow_tabs && line.trim_start_with(|c| c == '\t') == tag) {
                // found the tag
                heredoc_end = Some((prev_newline, i));
                break
            }
            prev_newline = i;
        }

        // the body is everything from heredoc_start+1 .. heredoc_end.0
        // the closing tag is at heredoc_end.0+1 .. heredoc_end.1
        // do the end first as it may shift indexes

        if let Some(heredoc_end) = heredoc_end {
            let range = heredoc_end.0+1 .. heredoc_end.1;
            let token = Token{
                kind: TokenKind::HeredocCloseTag,
                range: tokens[range.start].range.start .. tokens[range.end-1].range.end,
                children: None,
            };
            offset += range.end - range.start + 1;
            tokens.splice(range, [token]);
        }

        let kind = TokenKind::HeredocBody;
        let range = heredoc_start+1 .. heredoc_end.unwrap_or((tokens.len(), 0)).0;

        if !range.is_empty() {
            let new_start = tokens[range.start-1].range.end;
            let new_end = tokens.get(range.end).map_or(cmd.len(), |t| t.range.start);
            if allow_subst {
                let token = nest_tokens(tokens, range.start, range.end, kind);
                token.range = new_start .. new_end;
                for n in token.children.iter_mut().flatten() {
                    n.kind = TokenKind::None;
                }
            } else {
                let token = Token{
                    kind: TokenKind::HeredocBody,
                    range: new_start .. new_end,
                    children: None,
                };
                tokens.splice(range.clone(), [token]);
            }
            offset += range.end - range.start + 1;
        }

        if heredoc_end.is_none() {
            return false
        }
    }

    true
}

fn apply_custom_token(cmd: &BStr, options: ParserOptions, tokens: &mut Vec<Token>, start: usize, mut end: usize, range_offset: usize) {
    // detect subshells
    // this is inefficient but whatever

    let mut i = start;
    while i < end {

        let slice = &mut tokens[i..];
        let action = match *slice {

            // [$><=](...)
            [
                Token{kind: TokenKind::Token(token::String | token::Qstring | token::OutangProc | token::Inang | token::Equals), ..},
                Token{kind: TokenKind::Token(token::Inpar), ..},
  ref mut tok @ Token{kind: TokenKind::None | TokenKind::Lextok(lextok::STRING | lextok::LEXERR), ..},
                Token{kind: TokenKind::Token(token::Outpar), ..},
            ..] => Some((TokenKind::Substitution, Some(tok), 4)),

            // [$><=](...
            [
                Token{kind: TokenKind::Token(token::String | token::Qstring | token::OutangProc | token::Inang | token::Equals), ..},
                Token{kind: TokenKind::Token(token::Inpar), ..},
  ref mut tok @ Token{kind: TokenKind::None | TokenKind::Lextok(lextok::STRING | lextok::LEXERR), ..},
            ..] => Some((TokenKind::Substitution, Some(tok), 3)),

            // `...`
            [
                Token{kind: TokenKind::Token(token::Tick | token::Qtick), ..},
  ref mut tok @ Token{kind: TokenKind::None | TokenKind::Lextok(lextok::STRING | lextok::LEXERR), ..},
                Token{kind: TokenKind::Token(token::Tick | token::Qtick), ..},
            ..] => Some((TokenKind::Substitution, Some(tok), 3)),

            // `...
            [
                Token{kind: TokenKind::Token(token::Tick | token::Qtick), ..},
  ref mut tok @ Token{kind: TokenKind::None | TokenKind::Lextok(lextok::STRING | lextok::LEXERR), ..},
            ..] => Some((TokenKind::Substitution, Some(tok), 2)),

            // (<|>|>>|<>|>\||>!|<&|>&|>&\|>&!|&>\||&>!|>>&|&>>|>>&\||>>&!|&>>\||&>>!) STRING
            [
                Token{kind: TokenKind::Lextok(lextok::OUTANG | lextok::OUTANGBANG | lextok::DOUTANG | lextok::DOUTANGBANG | lextok::INANG | lextok::INOUTANG | lextok::INANGAMP | lextok::OUTANGAMP | lextok::AMPOUTANG | lextok::OUTANGAMPBANG | lextok::DOUTANGAMP | lextok::DOUTANGAMPBANG | lextok::TRINANG), ..},
                Token{kind: TokenKind::None | TokenKind::Lextok(lextok::STRING | lextok::LEXERR), ..},
            ..] => Some((TokenKind::Redirect, None, 2)),

            // function STRING () [[{]
            [
                Token{kind: TokenKind::Lextok(lextok::FUNC), ..},
                Token{kind: TokenKind::None | TokenKind::Lextok(lextok::STRING | lextok::LEXERR), ..},
                Token{kind: TokenKind::Lextok(lextok::INOUTPAR), ..},
                Token{kind: TokenKind::Lextok(lextok::INBRACE | lextok::INPAR), ..},
            ..] => Some((TokenKind::Function, None, 4)),
            // function {
            [
                Token{kind: TokenKind::Lextok(lextok::FUNC), ..},
                Token{kind: TokenKind::Lextok(lextok::INBRACE), ..},
            ..] => Some((TokenKind::Function, None, 2)),
            // STRING () [[{]
            [
                Token{kind: TokenKind::None | TokenKind::Lextok(lextok::STRING | lextok::LEXERR), ..},
                Token{kind: TokenKind::Lextok(lextok::INOUTPAR), ..},
                Token{kind: TokenKind::Lextok(lextok::INBRACE | lextok::INPAR), ..},
            ..] => Some((TokenKind::Function, None, 3)),

            _ => None,
        };

        if let Some((kind, tok, mut len)) = action {

            match kind {
                TokenKind::Substitution => {
                    let tok = tok.unwrap();
                    let cmd = tok.as_str(cmd, range_offset);
                    // tok.children = Some(parse_internal(cmd.to_owned(), options, tok.range.start, None).1);
                },
                TokenKind::Function => {
                    // yuck
                    // find the last bracket
                    let mut bracket_count = 0;
                    let mut func_len = len;
                    for (j, tok) in tokens[i+len-1..end].iter().enumerate() {
                        match tok.kind {
                            TokenKind::Lextok(lextok::INBRACE | lextok::INPAR) => bracket_count += 1,
                            TokenKind::Lextok(lextok::OUTBRACE | lextok::OUTPAR) => {
                                bracket_count -= 1;
                                if bracket_count == 0 {
                                    func_len += j;
                                    break
                                }
                            },
                            _ => (),
                        }
                    }
                    if bracket_count > 0 {
                        func_len = end - i;
                    }
                    let body = nest_tokens(tokens, i+len, i+func_len-1, TokenKind::None).children.as_mut().unwrap();
                    end -= func_len - len - 1;
                    len += 2; // body and end bracket
                    // look for more functions
                    let body_len = body.len();
                    apply_custom_token(cmd, options, body, 0, body_len, range_offset);
                },
                _ => (),
            }

            nest_tokens(tokens, i, i+len, kind);
            end -= len - 1;
        }

        i += 1;
    }

}

fn nest_tokens(tokens: &mut Vec<Token>, start: usize, end: usize, kind: TokenKind) -> &mut Token {
    let super_token = Token{
        kind,
        range: tokens[start].range.start .. tokens[end-1].range.end,
        children: None,
    };
    let children = tokens.splice(start..end, [super_token]).collect();
    tokens[start].children = Some(children);
    &mut tokens[start]
}
