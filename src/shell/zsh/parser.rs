use serde::{Deserialize};
use std::ops::Range;
use std::ffi::CStr;
use std::ptr::null_mut;
use bstr::{BString, BStr, ByteSlice};
use super::bindings::{Meta, token, lextok};

#[derive(Default, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct ParserOptions {
    comments: Option<bool>,
    custom: bool,
}

#[derive(Debug, Clone)]
pub enum TokenKind {
    Lextok(lextok),
    Token(token),
    Substitution,
    Redirect,
    Function,
    Comment,
}

impl std::fmt::Display for TokenKind {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            TokenKind::Lextok(k) => write!(fmt, "{k:?}"),
            TokenKind::Token(k) => write!(fmt, "{k:?}"),
            TokenKind::Substitution => write!(fmt, "substitution"),
            TokenKind::Redirect => write!(fmt, "redirect"),
            TokenKind::Function => write!(fmt, "function"),
            TokenKind::Comment => write!(fmt, "comment"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub range: Range<usize>,
    pub kind: Option<TokenKind>,
    pub nested: Option<Vec<Token>>,
}

impl Token {
    pub fn as_str<'a>(&self, cmd: &'a BStr) -> &'a BStr {
        &cmd[self.range.clone()]
    }

    pub fn remove_dummy_from_nested(&mut self, len: usize) {
        let mut tok = self;
        while let Some(nested) = tok.nested.as_mut() && let Some(last) = nested.last_mut() {
            last.range.end -= len;
            if last.range.is_empty() {
                nested.pop();
                break
            }
            tok = nested.last_mut().unwrap();
        }
    }
}

fn find_str(needle: &BStr, haystack: &BStr, start: usize) -> Option<Range<usize>> {
    let start = start + match needle.as_bytes() {
        b";" => haystack[start..].iter().position(|&c| c == b';' || c == b'\n'),
        // b"&|" | b"&!"
        _ => haystack[start..].find(needle),
    }?;

    Some(start .. start + needle.len())
}

pub fn parse(cmd: BString, options: ParserOptions) -> (bool, Vec<Token>) {
    // we add some at the end to detect if the command line is actually complete
    let dummy = b" x";
    let mut cmd = cmd.to_owned();
    cmd.extend(dummy);

    let (mut complete, mut tokens) = parse_internal(cmd.as_ref(), options, 0);

    if let Some(last) = tokens.last_mut() {
        debug_assert_eq!(last.range.end, cmd.len());

        // if the last token is just the dummy, pop it
        if last.range.start == cmd.len() - 1 {
            tokens.pop();
        } else {
            // otherwise it must be joined onto an incomplete token
            if !matches!(last.kind, Some(TokenKind::Comment)) {
                complete = false;
            }
            last.range.end -= dummy.len();
            if last.range.is_empty() {
                tokens.pop();
            } else {
                last.remove_dummy_from_nested(dummy.len());
            }
        }
    } else {
        // no tokens???
        complete = false;
    }

    (complete, tokens)
}

fn parse_internal(cmd: &BStr, options: ParserOptions, range_offset: usize) -> (bool, Vec<Token>) {
    let ptr = super::metafy(cmd);
    let metafied = unsafe{ CStr::from_ptr(ptr) };
    let metalen = metafied.count_bytes();
    let mut complete = true;
    let mut tokens = vec![];
    let mut start = 0;

    let mut push_token = |tokens: &mut Vec<Token>, tokstr: &[u8], kind: Option<TokenKind>, has_meta| {
        let range = if has_meta {
            let mut tokstr = tokstr.to_owned();
            super::unmetafy_owned(&mut tokstr);
            find_str(BStr::new(tokstr.as_slice()), cmd, start).unwrap()
        } else {
            find_str(BStr::new(tokstr), cmd, start).unwrap()
        };
        start = range.end;
        tokens.push(Token{range: range.start + range_offset .. range.end + range_offset, kind, nested: None});
    };

    // do similar to bufferwords
    unsafe {
        zsh_sys::zcontext_save();
        zsh_sys::inpush(ptr, 0, null_mut());
        zsh_sys::zlemetall = cmd.len() as _;
        zsh_sys::zlemetacs = zsh_sys::zlemetall;
        zsh_sys::strinbeg(0);
        let old_noerrs = super::set_error_verbosity(super::ErrorVerbosity::Ignore);
        let old_noaliases = zsh_sys::noaliases;
        zsh_sys::noaliases = 1;

        let old_lexflags = zsh_sys::lexflags;
        let mut new_lexflags = zsh_sys::LEXFLAGS_ACTIVE | zsh_sys::LEXFLAGS_ZLE;
        if options.comments.unwrap_or(super::isset(zsh_sys::INTERACTIVECOMMENTS as _)) {
            new_lexflags |= zsh_sys::LEXFLAGS_COMMENTS_KEEP;
        }
        zsh_sys::lexflags = new_lexflags as _;

        // ztokens has the wrong length, so use pointer arithmetic instead
        #[allow(static_mut_refs)]
        let ztokens = zsh_sys::ztokens.as_ptr();

        loop {
            zsh_sys::ctxtlex();

            if zsh_sys::tok == zsh_sys::lextok_ENDINPUT {
                break
            }

            let kind: Option<TokenKind> = num::FromPrimitive::from_u32(zsh_sys::tok).map(TokenKind::Lextok);

            if zsh_sys::tokstr.is_null() {
                let range = metalen - 1 - zsh_sys::wordbeg as usize .. metalen - zsh_sys::inbufct as usize;
                let bytes = &metafied.to_bytes()[range];
                let has_meta = bytes.contains(&Meta);
                push_token(&mut tokens, bytes, kind, has_meta);

            } else {
                // tokstr metafied and tokenized
                // let's go through the tokens

                let tokstr = CStr::from_ptr(zsh_sys::tokstr).to_bytes();

                let mut slice_start = 0;
                let mut meta = false;
                let mut has_meta = false;

                let mut nested = vec![];
                for (i, c) in tokstr.iter().enumerate() {
                    if meta {
                        meta = false;
                    } else if *c == Meta {
                        meta = true;
                        has_meta = true;
                    } else if *c >= token::Pound as _ && *c < token::Nularg as _ { // token

                        if i > slice_start {
                            push_token(&mut nested, &tokstr[slice_start..i], None, has_meta);
                        }
                        has_meta = false;
                        slice_start = i + 1;

                        let kind: Option<TokenKind> = num::FromPrimitive::from_u8(*c).map(TokenKind::Token);
                        let c = [*ztokens.add((*c - token::Pound as u8) as usize) as u8];
                        push_token(&mut nested, &c[..], kind, false);
                    }
                }

                let kind = if matches!(kind, Some(TokenKind::Lextok(lextok::STRING))) && tokstr.starts_with(b"#") {
                    Some(TokenKind::Comment)
                } else {
                    kind
                };

                if slice_start == 0 {
                    // no inner tokens
                    push_token(&mut tokens, tokstr, kind, has_meta);

                } else {
                    if tokstr.len() > slice_start {
                        push_token(&mut nested, &tokstr[slice_start..], None, has_meta);
                    }
                    if options.custom {
                        let len = nested.len();
                        apply_custom_token(cmd, options, &mut nested, 0, len, range_offset);
                    }
                    let range = nested[0].range.start .. nested.last().unwrap().range.end;
                    tokens.push(Token{range, kind, nested: Some(nested)});
                }
            }

            if zsh_sys::tok == zsh_sys::lextok_LEXERR || (zsh_sys::errflag & zsh_sys::errflag_bits_ERRFLAG_INT as i32) > 0 {
                complete = false;
                break
            }

        }

        // restore
        zsh_sys::lexflags = old_lexflags;
        zsh_sys::strinend();
        zsh_sys::inpop();
        zsh_sys::errflag &= !zsh_sys::errflag_bits_ERRFLAG_ERROR as i32;
        zsh_sys::noaliases = old_noaliases;
        super::set_error_verbosity(old_noerrs);
        zsh_sys::zcontext_restore();
    }

    if options.custom {
        let len = tokens.len();
        apply_custom_token(cmd, options, &mut tokens, 0, len, range_offset);
    }

    (complete, tokens)
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
                Token{kind: Some(TokenKind::Token(token::String | token::Qstring | token::OutangProc | token::Inang | token::Equals)), ..},
                Token{kind: Some(TokenKind::Token(token::Inpar)), ..},
  ref mut tok @ Token{kind: None | Some(TokenKind::Lextok(lextok::STRING | lextok::LEXERR)), ..},
                Token{kind: Some(TokenKind::Token(token::Outpar)), ..},
            ..] => Some((TokenKind::Substitution, Some(tok), 4)),

            // [$><=](...
            [
                Token{kind: Some(TokenKind::Token(token::String | token::Qstring | token::OutangProc | token::Inang | token::Equals)), ..},
                Token{kind: Some(TokenKind::Token(token::Inpar)), ..},
  ref mut tok @ Token{kind: None | Some(TokenKind::Lextok(lextok::STRING | lextok::LEXERR)), ..},
            ..] => Some((TokenKind::Substitution, Some(tok), 3)),

            // `...`
            [
                Token{kind: Some(TokenKind::Token(token::Tick | token::Qtick)), ..},
  ref mut tok @ Token{kind: None | Some(TokenKind::Lextok(lextok::STRING | lextok::LEXERR)), ..},
                Token{kind: Some(TokenKind::Token(token::Tick | token::Qtick)), ..},
            ..] => Some((TokenKind::Substitution, Some(tok), 3)),

            // `...
            [
                Token{kind: Some(TokenKind::Token(token::Tick | token::Qtick)), ..},
  ref mut tok @ Token{kind: None | Some(TokenKind::Lextok(lextok::STRING | lextok::LEXERR)), ..},
            ..] => Some((TokenKind::Substitution, Some(tok), 2)),

            // (<|>|>>|<>|>\||>!|<&|>&|>&\|>&!|&>\||&>!|>>&|&>>|>>&\||>>&!|&>>\||&>>!) STRING
            [
                Token{kind: Some(TokenKind::Lextok(lextok::OUTANG | lextok::OUTANGBANG | lextok::DOUTANG | lextok::DOUTANGBANG | lextok::INANG | lextok::INOUTANG | lextok::INANGAMP | lextok::OUTANGAMP | lextok::AMPOUTANG | lextok::OUTANGAMPBANG | lextok::DOUTANGAMP | lextok::DOUTANGAMPBANG | lextok::TRINANG)), ..},
                Token{kind: None | Some(TokenKind::Lextok(lextok::STRING | lextok::LEXERR)), ..},
            ..] => Some((TokenKind::Redirect, None, 2)),

            // function STRING () [[{]
            [
                Token{kind: Some(TokenKind::Lextok(lextok::FUNC)), ..},
                Token{kind: None | Some(TokenKind::Lextok(lextok::STRING | lextok::LEXERR)), ..},
                Token{kind: Some(TokenKind::Lextok(lextok::INOUTPAR)), ..},
                Token{kind: Some(TokenKind::Lextok(lextok::INBRACE | lextok::INPAR)), ..},
            ..] => Some((TokenKind::Function, None, 4)),
            // function {
            [
                Token{kind: Some(TokenKind::Lextok(lextok::FUNC)), ..},
                Token{kind: Some(TokenKind::Lextok(lextok::INBRACE)), ..},
            ..] => Some((TokenKind::Function, None, 2)),
            // STRING () [[{]
            [
                Token{kind: None | Some(TokenKind::Lextok(lextok::STRING | lextok::LEXERR)), ..},
                Token{kind: Some(TokenKind::Lextok(lextok::INOUTPAR)), ..},
                Token{kind: Some(TokenKind::Lextok(lextok::INBRACE | lextok::INPAR)), ..},
            ..] => Some((TokenKind::Function, None, 3)),

            _ => None,
        };

        if let Some((kind, tok, mut len)) = action {

            match kind {
                TokenKind::Substitution => {
                    let tok = tok.unwrap();
                    let cmd = &cmd[tok.range.start - range_offset .. tok.range.end - range_offset];
                    tok.nested = Some(parse_internal(cmd, options, tok.range.start).1);
                },
                TokenKind::Function => {
                    // yuck
                    // find the last bracket
                    let mut bracket_count = 0;
                    let mut func_len = len;
                    for (j, tok) in tokens[i+len-1..end].iter().enumerate() {
                        match tok.kind {
                            Some(TokenKind::Lextok(lextok::INBRACE | lextok::INPAR)) => bracket_count += 1,
                            Some(TokenKind::Lextok(lextok::OUTBRACE | lextok::OUTPAR)) => {
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
                    let body = nest_tokens(tokens, i+len, i+func_len-1, None).nested.as_mut().unwrap();
                    end -= func_len - len - 1;
                    len += 2; // body and end bracket
                    // look for more functions
                    let body_len = body.len();
                    apply_custom_token(cmd, options, body, 0, body_len, range_offset);
                },
                _ => (),
            }

            nest_tokens(tokens, i, i+len, Some(kind));
            end -= len - 1;
        }

        i += 1;
    }

}

fn nest_tokens(tokens: &mut Vec<Token>, start: usize, end: usize, kind: Option<TokenKind>) -> &mut Token {
    let super_token = Token{
        kind,
        range: tokens[start].range.start .. tokens[end-1].range.end,
        nested: None,
    };
    let nested = tokens.splice(start..end, [super_token]).collect();
    tokens[start].nested = Some(nested);
    &mut tokens[start]
}
