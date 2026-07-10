#[derive(Debug, Clone, PartialEq)]
pub struct Username(String);
impl Username {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// The structured, localisable rejection reason. Never a message string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsernameError {
    TooShort { min: u32, actual: u32 },
    TooLong { max: u32, actual: u32 },
    InvalidChars,
}
impl ::bolted_core::Value for Username {
    type Raw = String;
    type Error = UsernameError;
    fn try_new(__raw: Self::Raw) -> ::core::result::Result<Self, Self::Error> {
        let __raw = __raw.trim().to_owned();
        let __len = __raw.chars().count() as u32;
        if __len < 3u32 {
            return Err(UsernameError::TooShort {
                min: 3u32,
                actual: __len,
            });
        }
        if __len > 20u32 {
            return Err(UsernameError::TooLong {
                max: 20u32,
                actual: __len,
            });
        }
        if !ascii_alnum_underscore(&__raw) {
            return Err(UsernameError::InvalidChars);
        }
        Ok(Username(__raw))
    }
    fn into_raw(self) -> Self::Raw {
        self.0
    }
    fn constraints() -> &'static [::bolted_core::Constraint] {
        &[
            ::bolted_core::Constraint::LenChars {
                min: 3u32,
                max: 20u32,
            },
            ::bolted_core::Constraint::Custom("ascii_alnum_underscore"),
        ]
    }
}
impl ::core::convert::From<UsernameError> for ::bolted_core::ErrorData {
    fn from(__e: UsernameError) -> Self {
        match __e {
            UsernameError::TooShort { min, actual } => {
                ::bolted_core::ErrorData {
                    key: "too_short",
                    params: vec![
                        ("min", ::std::string::ToString::to_string(& min)), ("actual",
                        ::std::string::ToString::to_string(& actual)),
                    ],
                }
            }
            UsernameError::TooLong { max, actual } => {
                ::bolted_core::ErrorData {
                    key: "too_long",
                    params: vec![
                        ("max", ::std::string::ToString::to_string(& max)), ("actual",
                        ::std::string::ToString::to_string(& actual)),
                    ],
                }
            }
            UsernameError::InvalidChars => ::bolted_core::ErrorData::new("invalid_chars"),
        }
    }
}
