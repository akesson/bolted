#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub username: Username,
    pub name: PersonName,
    pub email: Email,
    pub availability: DateRange,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileField {
    Username,
    Name,
    Email,
    Availability,
}
impl ProfileField {
    /// `Required` is prepended because every entity field is non-optional — a *field*-level
    /// judgement no value type can make (D13) — followed by the value type's own intrinsics.
    pub fn constraints(self) -> ::std::vec::Vec<::bolted_core::Constraint> {
        let mut out = vec![::bolted_core::Constraint::Required];
        let intrinsic: &'static [::bolted_core::Constraint] = match self {
            ProfileField::Username => <Username as ::bolted_core::Value>::constraints(),
            ProfileField::Name => <PersonName as ::bolted_core::Value>::constraints(),
            ProfileField::Email => <Email as ::bolted_core::Value>::constraints(),
            ProfileField::Availability => {
                <DateRange as ::bolted_core::Value>::constraints()
            }
        };
        out.extend_from_slice(intrinsic);
        out
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileCheck {
    UsernameUnique,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileStash {
    pub username: ::bolted_core::FieldStash<<Username as ::bolted_core::Value>::Raw>,
    pub name: ::bolted_core::FieldStash<<PersonName as ::bolted_core::Value>::Raw>,
    pub email: ::bolted_core::FieldStash<<Email as ::bolted_core::Value>::Raw>,
    pub availability: ::bolted_core::FieldStash<
        <DateRange as ::bolted_core::Value>::Raw,
    >,
    /// The store version this draft was last based on.
    pub base_version: u64,
    /// A draft orphaned before the process died stays orphaned (C11).
    pub orphaned: bool,
}
pub struct ProfileDraft {
    pub username: ::bolted_core::Field<Username>,
    pub name: ::bolted_core::Field<PersonName>,
    pub email: ::bolted_core::Field<Email>,
    pub availability: ::bolted_core::Field<DateRange>,
    username_check: ::bolted_core::SingleFlight<
        ::core::result::Result<(), ::bolted_core::ErrorData>,
    >,
    status: ::bolted_core::DraftStatus,
    base_version: u64,
}
pub type ProfileStore = ::bolted_core::Store<ProfileDraft>;
impl ProfileDraft {
    fn bolted_guard<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let __before_username = self.username.value().cloned();
        let __out = f(self);
        if self.username.value() != __before_username.as_ref() {
            self.username_check.reset();
        }
        __out
    }
}
impl ProfileDraft {
    pub fn try_set_username(
        &mut self,
        raw: <Username as ::bolted_core::Value>::Raw,
    ) -> ::core::result::Result<(), <Username as ::bolted_core::Value>::Error> {
        self.bolted_guard(|__d| __d.username.try_set(raw))
    }
    pub fn try_set_name(
        &mut self,
        raw: <PersonName as ::bolted_core::Value>::Raw,
    ) -> ::core::result::Result<(), <PersonName as ::bolted_core::Value>::Error> {
        self.name.try_set(raw)
    }
    pub fn try_set_email(
        &mut self,
        raw: <Email as ::bolted_core::Value>::Raw,
    ) -> ::core::result::Result<(), <Email as ::bolted_core::Value>::Error> {
        self.email.try_set(raw)
    }
    pub fn try_set_availability(
        &mut self,
        raw: <DateRange as ::bolted_core::Value>::Raw,
    ) -> ::core::result::Result<(), <DateRange as ::bolted_core::Value>::Error> {
        self.availability.try_set(raw)
    }
}
#[doc(hidden)]
pub trait ProfileRules {
    fn rules(&self) -> ::std::vec::Vec<::bolted_core::RuleViolation<ProfileField>>;
}
impl ::bolted_core::Draft for ProfileDraft {
    type Entity = Profile;
    type FieldId = ProfileField;
    fn status(&self) -> ::bolted_core::DraftStatus {
        self.status
    }
    fn base_version(&self) -> u64 {
        self.base_version
    }
    fn dirty_fields(&self) -> ::std::vec::Vec<ProfileField> {
        let mut out = ::std::vec::Vec::new();
        if self.username.is_dirty() {
            out.push(ProfileField::Username);
        }
        if self.name.is_dirty() {
            out.push(ProfileField::Name);
        }
        if self.email.is_dirty() {
            out.push(ProfileField::Email);
        }
        if self.availability.is_dirty() {
            out.push(ProfileField::Availability);
        }
        out
    }
    fn conflicts(&self) -> ::std::vec::Vec<ProfileField> {
        let mut out = ::std::vec::Vec::new();
        if self.username.is_conflicted() {
            out.push(ProfileField::Username);
        }
        if self.name.is_conflicted() {
            out.push(ProfileField::Name);
        }
        if self.email.is_conflicted() {
            out.push(ProfileField::Email);
        }
        if self.availability.is_conflicted() {
            out.push(ProfileField::Availability);
        }
        out
    }
    fn validate(&self) -> ::bolted_core::ValidationReport<ProfileField> {
        let mut report = ::bolted_core::ValidationReport::new();
        if let Some(e) = self.username.required_error() {
            report.field_errors.push((ProfileField::Username, e));
        }
        if let Some(e) = self.name.required_error() {
            report.field_errors.push((ProfileField::Name, e));
        }
        if let Some(e) = self.email.required_error() {
            report.field_errors.push((ProfileField::Email, e));
        }
        if let Some(e) = self.availability.required_error() {
            report.field_errors.push((ProfileField::Availability, e));
        }
        report.rule_errors.extend(ProfileRules::rules(self));
        if let Some(v) = self
            .username_check
            .violation(
                "username_unique",
                ProfileField::Username,
                self.username.is_dirty(),
                "username_check_pending",
                "username_check_required",
            )
        {
            report.rule_errors.push(v);
        }
        report
    }
    fn resolve_keep_mine(&mut self, field: ProfileField) {
        self.bolted_guard(|__d| match field {
            ProfileField::Username => __d.username.resolve_keep_mine(),
            ProfileField::Name => __d.name.resolve_keep_mine(),
            ProfileField::Email => __d.email.resolve_keep_mine(),
            ProfileField::Availability => __d.availability.resolve_keep_mine(),
        })
    }
    fn resolve_take_theirs(&mut self, field: ProfileField) {
        self.bolted_guard(|__d| match field {
            ProfileField::Username => __d.username.resolve_take_theirs(),
            ProfileField::Name => __d.name.resolve_take_theirs(),
            ProfileField::Email => __d.email.resolve_take_theirs(),
            ProfileField::Availability => __d.availability.resolve_take_theirs(),
        })
    }
    fn commit(
        self,
    ) -> ::core::result::Result<
        Profile,
        (Self, ::bolted_core::CommitError<ProfileField>),
    > {
        if let Some(e) = ::bolted_core::commit_gates(&self) {
            return Err((self, e));
        }
        match (
            self.username.value().cloned(),
            self.name.value().cloned(),
            self.email.value().cloned(),
            self.availability.value().cloned(),
        ) {
            (Some(username), Some(name), Some(email), Some(availability)) => {
                Ok(Profile {
                    username,
                    name,
                    email,
                    availability,
                })
            }
            _ => {
                let report = <Self as ::bolted_core::Draft>::validate(&self);
                Err((self, ::bolted_core::CommitError::Validation(report)))
            }
        }
    }
}
impl ::bolted_core::StoreDraft for ProfileDraft {
    fn from_canonical(base: Option<&Profile>, base_version: u64) -> Self {
        match base {
            Some(__e) => {
                ProfileDraft {
                    username: ::bolted_core::Field::from_base(__e.username.clone()),
                    name: ::bolted_core::Field::from_base(__e.name.clone()),
                    email: ::bolted_core::Field::from_base(__e.email.clone()),
                    availability: ::bolted_core::Field::from_base(
                        __e.availability.clone(),
                    ),
                    username_check: ::bolted_core::SingleFlight::new(),
                    status: ::bolted_core::DraftStatus::Live,
                    base_version,
                }
            }
            None => {
                ProfileDraft {
                    username: ::bolted_core::Field::new_unset(),
                    name: ::bolted_core::Field::new_unset(),
                    email: ::bolted_core::Field::new_unset(),
                    availability: ::bolted_core::Field::new_unset(),
                    username_check: ::bolted_core::SingleFlight::new(),
                    status: ::bolted_core::DraftStatus::Live,
                    base_version,
                }
            }
        }
    }
    fn rebase(&mut self, entity: &Profile, version: u64) {
        if matches!(self.status, ::bolted_core::DraftStatus::Orphaned) {
            return;
        }
        self.bolted_guard(|__d| {
            __d.username.rebase(entity.username.clone());
            __d.name.rebase(entity.name.clone());
            __d.email.rebase(entity.email.clone());
            __d.availability.rebase(entity.availability.clone());
        });
        self.base_version = version;
    }
    fn orphan(&mut self) {
        self.status = ::bolted_core::DraftStatus::Orphaned;
    }
    fn is_based(&self) -> bool {
        self.username.base().is_some() || self.name.base().is_some()
            || self.email.base().is_some() || self.availability.base().is_some()
    }
}
impl ::bolted_core::Stashable for ProfileDraft {
    type Stash = ProfileStash;
    fn stash(&self) -> ProfileStash {
        ProfileStash {
            username: self.username.stash(),
            name: self.name.stash(),
            email: self.email.stash(),
            availability: self.availability.stash(),
            base_version: self.base_version,
            orphaned: matches!(self.status, ::bolted_core::DraftStatus::Orphaned),
        }
    }
    fn from_stash(stash: &ProfileStash) -> Self {
        ProfileDraft {
            username: ::bolted_core::Field::from_stash(&stash.username),
            name: ::bolted_core::Field::from_stash(&stash.name),
            email: ::bolted_core::Field::from_stash(&stash.email),
            availability: ::bolted_core::Field::from_stash(&stash.availability),
            username_check: ::bolted_core::SingleFlight::new(),
            status: if stash.orphaned {
                ::bolted_core::DraftStatus::Orphaned
            } else {
                ::bolted_core::DraftStatus::Live
            },
            base_version: stash.base_version,
        }
    }
}
impl ::bolted_core::Checked for ProfileDraft {
    type CheckId = ProfileCheck;
    fn begin_check(&mut self, check: ProfileCheck) -> ::bolted_core::CheckToken {
        match check {
            ProfileCheck::UsernameUnique => self.username_check.begin(),
        }
    }
    fn complete_check(
        &mut self,
        check: ProfileCheck,
        token: ::bolted_core::CheckToken,
        verdict: ::core::result::Result<(), ::bolted_core::ErrorData>,
    ) -> bool {
        match check {
            ProfileCheck::UsernameUnique => self.username_check.complete(token, verdict),
        }
    }
    fn check_state(
        &self,
        check: ProfileCheck,
    ) -> &::bolted_core::CheckState<
        ::core::result::Result<(), ::bolted_core::ErrorData>,
    > {
        match check {
            ProfileCheck::UsernameUnique => self.username_check.state(),
        }
    }
    fn check_pins(check: ProfileCheck) -> ProfileField {
        match check {
            ProfileCheck::UsernameUnique => ProfileField::Username,
        }
    }
}
