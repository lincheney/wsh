use crate::shell::{MetaString};
use bstr::{BString, BStr, ByteSlice};
use regex::bytes::Regex;


#[derive(Debug)]
pub enum RemovalTrigger {
    Default(BString),
    Chars{regex: Regex, match_empty: bool},
    Function{name: MetaString, len: usize},
}

#[derive(Debug)]
pub struct Suffix {
    pub removal_trigger: RemovalTrigger,
    pub byte_len: usize,
}

impl Suffix {

    pub fn matches(&self, buf: Option<&BStr>) -> bool {
        match &self.removal_trigger {
            RemovalTrigger::Function{..} => true,
            RemovalTrigger::Default(suf) => {
                if let Some(buf) = buf {
                    // matches if whitespace
                    if buf.trim_start().len() != buf.len() {
                        return true
                    }
                    // matches if char is same as suffix
                    return buf.graphemes().next().map(|x| x.into()) == Some(suf.as_bstr())
                } else {
                    // matches if no changes
                    true
                }
            },
            RemovalTrigger::Chars{regex, match_empty} => {
                if let Some(buf) = buf {
                    regex.is_match(buf)
                } else {
                    // matches if no changes
                    *match_empty
                }
            },
        }
    }

    pub fn try_into_func(self) -> Result<(MetaString, usize), Self> {
        match self {
            Self{ removal_trigger: RemovalTrigger::Function{name, len}, .. } => Ok((name, len)),
            x => Err(x),
        }
    }

}
