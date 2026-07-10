impl ProfileDraft {
    fn corporate_email(&self) -> Result<(), ErrorData> {
        Ok(())
    }
}
impl ProfileRules for ProfileDraft {
    fn rules(&self) -> ::std::vec::Vec<::bolted_core::RuleViolation<ProfileField>> {
        let mut out = ::std::vec::Vec::new();
        if let Err(error) = self.corporate_email() {
            out.push(::bolted_core::RuleViolation {
                rule: "corporate_email",
                pins: vec![ProfileField::Email],
                error,
            });
        }
        out
    }
}
