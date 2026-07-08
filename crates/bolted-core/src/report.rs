//! Validation reports. Errors are DATA (a stable key + named params), never message strings —
//! shells localise. Field IDs are typed so a report is keyed to concrete fields.

/// A structured, localisable error: a stable `key` plus named `params`.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorData {
    pub key: &'static str,
    pub params: Vec<(&'static str, String)>,
}

impl ErrorData {
    /// A keyed error with no params.
    pub fn new(key: &'static str) -> Self {
        ErrorData {
            key,
            params: Vec::new(),
        }
    }
}

/// A tier-2 rule violation: which rule fired, which field IDs it pins its error to, and the data.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleViolation<FieldId> {
    pub rule: &'static str,
    pub pins: Vec<FieldId>,
    pub error: ErrorData,
}

/// The full outcome of validating a draft: tier-1 field errors and tier-2 rule errors.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport<FieldId> {
    /// Tier 1: fields that are `Invalid`, or `Unset` while required.
    pub field_errors: Vec<(FieldId, ErrorData)>,
    /// Tier 2: relational rule violations.
    pub rule_errors: Vec<RuleViolation<FieldId>>,
}

impl<FieldId> ValidationReport<FieldId> {
    pub fn new() -> Self {
        ValidationReport {
            field_errors: Vec::new(),
            rule_errors: Vec::new(),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.field_errors.is_empty() && self.rule_errors.is_empty()
    }
}

impl<FieldId> Default for ValidationReport<FieldId> {
    fn default() -> Self {
        Self::new()
    }
}
