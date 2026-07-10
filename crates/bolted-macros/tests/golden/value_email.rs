#[derive(Debug, Clone, PartialEq)]
pub struct Email(String);
impl Email {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// The structured, localisable rejection reason. Never a message string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmailError {
    Invalid,
}
impl ::bolted_core::Value for Email {
    type Raw = String;
    type Error = EmailError;
    fn try_new(__raw: Self::Raw) -> ::core::result::Result<Self, Self::Error> {
        let __raw = __raw.trim().to_owned();
        let __raw = __raw.to_lowercase();
        if !email(&__raw) {
            return Err(EmailError::Invalid);
        }
        Ok(Email(__raw))
    }
    fn into_raw(self) -> Self::Raw {
        self.0
    }
    fn constraints() -> &'static [::bolted_core::Constraint] {
        &[::bolted_core::Constraint::Custom("email")]
    }
}
impl ::core::convert::From<EmailError> for ::bolted_core::ErrorData {
    fn from(__e: EmailError) -> Self {
        match __e {
            EmailError::Invalid => ::bolted_core::ErrorData::new("invalid_email"),
        }
    }
}
