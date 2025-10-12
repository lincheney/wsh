use std::ops::Range;
use std::ffi::CStr;
use std::ptr::null_mut;
use bstr::{BStr, ByteSlice};
use super::bindings;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TokenKind {
    Lextok(bindings::lextok),
    Token(bindings::token),
    Comment,
}

#[derive(Debug)]
pub struct Token {
    pub range: Range<usize>,
    kind: Option<TokenKind>,
}

impl Token {
    pub fn as_str<'a>(&self, cmd: &'a BStr) -> &'a BStr {
        &cmd[self.range.clone()]
    }

    pub fn kind_as_str(&self) -> Option<String> {
        Some(match self.kind? {
            TokenKind::Lextok(k) => format!("{:?}", k),
            TokenKind::Token(k) => format!("{:?}", k),
            TokenKind::Comment => "comment".into(),
        })
    }
}

fn find_str(needle: &BStr, haystack: &BStr, start: usize) -> Option<Range<usize>> {
    let start = start + if needle == b";" {
        haystack[start..].iter().position(|&c| c == b';' || c == b'\n')
    } else {
        haystack[start..].find(needle)
    }?;

    Some(start .. start + needle.len())
}

pub fn parse(cmd: &BStr, recursive: bool) -> (bool, Vec<Token>) {
    // we add some at the end to detect if the command line is actually complete
    let dummy = b" x";
    let mut cmd = cmd.to_owned();
    cmd.extend(dummy);

    let (mut complete, mut tokens) = _parse(cmd.as_ref(), recursive);

    if let Some(last) = tokens.last_mut() {
        debug_assert!(last.range.end == cmd.len());

        // if the last token is just the dummy, pop it
        if last.range.start == cmd.len() - 1 {
            tokens.pop();
        } else {
            // otherwise it must be joined onto an incomplete token
            if last.kind != Some(TokenKind::Comment) {
                complete = false;
            }
            last.range.end -= dummy.len();
            if last.range.is_empty() {
                tokens.pop();
            }
        }
    } else {
        // no tokens???
        complete = false;
    }

    (complete, tokens)
}

fn _parse(cmd: &BStr, recursive: bool) -> (bool, Vec<Token>) {
    let ptr = super::metafy(cmd);
    let mut complete = true;
    let mut tokens = vec![];
    let mut start = 0;

    macro_rules! push_token {
        ($tokstr:expr, $kind:expr, $has_meta:expr) => (
            let range = if $has_meta {
                let mut tokstr = $tokstr.to_owned();
                super::unmetafy_owned(&mut tokstr);
                find_str(BStr::new(tokstr.as_slice()), cmd.as_ref(), start).unwrap()
            } else {
                find_str(BStr::new($tokstr), cmd.as_ref(), start).unwrap()
            };
            start = range.end;
            tokens.push(Token{range, kind: $kind});
        )
    }

    // do similar to bufferwords
    unsafe {
        zsh_sys::zcontext_save();
        zsh_sys::inpush(ptr, 0, null_mut());
        zsh_sys::zlemetall = cmd.len() as _;
        zsh_sys::zlemetacs = zsh_sys::zlemetall;
        zsh_sys::strinbeg(0);
        let old_noaliases = zsh_sys::noaliases;
        zsh_sys::noaliases = 1;

        let lexflags = zsh_sys::lexflags;
        zsh_sys::lexflags = (zsh_sys::LEXFLAGS_ACTIVE | zsh_sys::LEXFLAGS_COMMENTS_KEEP) as _;

        // ztokens has the wrong length, so use pointer arithmetic instead
        #[allow(static_mut_refs)]
        let ztokens = zsh_sys::ztokens.as_ptr();

        loop {
            zsh_sys::ctxtlex();

            if zsh_sys::tok == zsh_sys::lextok_ENDINPUT {
                break
            }

            let mut kind: Option<TokenKind> = num::FromPrimitive::from_u32(zsh_sys::tok).map(TokenKind::Lextok);

            if zsh_sys::tokstr.is_null() {
                // no tokstr, so get string from tokstring table

                #[allow(static_mut_refs)]
                if let Some(tokstr) = zsh_sys::tokstrings.get(zsh_sys::tok as usize).filter(|t| !t.is_null()) {
                    let tokstr = CStr::from_ptr(*tokstr).to_bytes();
                    push_token!(tokstr, kind, false);

                } else {
                    // TODO what am i meant to do here?
                }

            } else {
                // tokstr metafied and tokenized
                // let's go through the tokens

                let tokstr = CStr::from_ptr(zsh_sys::tokstr).to_bytes();

                if kind == Some(TokenKind::Lextok(bindings::lextok::STRING)) && tokstr.starts_with(b"#") {
                    kind = Some(TokenKind::Comment);
                }

                let mut slice_start = 0;
                let mut meta = false;
                let mut has_meta = false;

                for (i, c) in tokstr.iter().enumerate() {
                    if meta {
                        meta = false;
                    } else if *c == bindings::Meta {
                        meta = true;
                        has_meta = true;
                    } else if *c >= bindings::token::Pound as _ && *c < bindings::token::Nularg as _ { // token

                        if i > slice_start {
                            push_token!(&tokstr[slice_start..i], kind, has_meta);
                        }
                        has_meta = false;
                        slice_start = i + 1;

                        let kind: Option<TokenKind> = num::FromPrimitive::from_u8(*c).map(TokenKind::Token);
                        let c = [*ztokens.add((*c - bindings::token::Pound as u8) as usize) as u8];
                        push_token!(&c[..], kind, false);
                    }
                }

                if tokstr.len() > slice_start {
                    push_token!(&tokstr[slice_start..], kind, has_meta);
                }
            }

            if zsh_sys::tok == zsh_sys::lextok_LEXERR || (zsh_sys::errflag & zsh_sys::errflag_bits_ERRFLAG_INT as i32) > 0 {
                complete = false;
                break
            }

        }

        // restore
        zsh_sys::lexflags = lexflags;
        zsh_sys::strinend();
        zsh_sys::inpop();
        zsh_sys::errflag &= !zsh_sys::errflag_bits_ERRFLAG_ERROR as i32;
        zsh_sys::noaliases = old_noaliases;
        zsh_sys::zcontext_restore();
    }

    // detect subshells
    // this is inefficient but whatever
    if recursive {
        let mut i = 2;
        while i < tokens.len() {
            let kinds = tokens[i-2].kind.zip(tokens[i-1].kind).zip(tokens[i].kind);
            if matches!(kinds, Some(((
                TokenKind::Token(
                    bindings::token::String // $(
                    | bindings::token::OutangProc // >(
                    | bindings::token::Inang // <(
                    | bindings::token::Equals // =(
                ),
                TokenKind::Token(bindings::token::Inpar),
            ),
                TokenKind::Lextok(bindings::lextok::STRING | bindings::lextok::LEXERR),
            ))) {

                let range = &tokens[i].range;
                let (_, mut subshell) = _parse(&cmd[range.clone()], true);
                for t in subshell.iter_mut() {
                    t.range.start += range.start;
                    t.range.end += range.start;
                }
                let replace = i ..= i;
                i += subshell.len() - 1;
                tokens.splice(replace, subshell);
            }

            i += 1;
        }
    }

    (complete, tokens)
}
