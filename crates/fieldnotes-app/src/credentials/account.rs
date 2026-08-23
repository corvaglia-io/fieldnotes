//! Which account each Field's credential authenticates as, and the diagnostic
//! for when they disagree.
//!
//! # The failure this exists to make visible
//!
//! Authenticating several Fields against one Microsoft 365 tenant means several
//! separate interactive browser flows, and **a browser silently reuses whatever
//! sign-in session is already open**. If one of those flows needed an
//! administrator (consenting for the organization, for instance), the credential
//! stored for that Field can authenticate the administrator while the others
//! authenticate the mailbox owner — and nothing about the stored credential said
//! so. That has already happened: collection failed with an error whose real
//! meaning was "the account you authenticated as has no mailbox", and
//! `fields status`, `fields auth`, and the sync report were all silent about the
//! three Fields being three different people.
//!
//! **The loud version of that mistake is the lucky one.** Had the wrong account
//! been an administrator who *does* have a mailbox, collection would have
//! succeeded and quietly filled the notebook with somebody else's mail. That is
//! a data-integrity problem, and it is invisible without this check.
//!
//! # Why this is a warning and not a refusal
//!
//! Differing accounts are not automatically wrong. Collecting a shared mailbox
//! alongside your own, or a delegated calendar, legitimately means two Fields
//! signed in as two principals. So this names what it found, names which Field
//! each account belongs to, and leaves the judgement to the person who can
//! actually make it.
//!
//! # This is never an authorization decision
//!
//! Every value here is a display label read out of an ID token
//! ([`fieldnotes_credentials::oauth::id_token`]). Nothing in this module — or
//! anywhere downstream of it — may grant access, deny access, choose a scope,
//! authorize a deletion, or select a credential on it.

use std::collections::BTreeMap;

use fieldnotes_store::{Notebook, list_field_configs};

use crate::error::AppError;

/// One recorded account, and every Field whose credential authenticates as it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountGroup {
    /// The recorded account.
    pub account: String,
    /// The Field IDs recorded against it, in ascending order.
    pub field_ids: Vec<String>,
}

/// More than one account is recorded across a notebook's Fields, and they do
/// not all agree.
///
/// Prominent, not fatal: see this module's documentation for why this is a
/// warning. Only constructed when there are at least two distinct accounts, so
/// its presence alone is the whole condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountMismatch {
    /// Every distinct recorded account with the Fields it belongs to, in
    /// ascending account order so output is deterministic.
    pub accounts: Vec<AccountGroup>,
}

impl AccountMismatch {
    /// Every distinct account named, in the same order as [`Self::accounts`].
    #[must_use]
    pub fn account_names(&self) -> Vec<&str> {
        self.accounts
            .iter()
            .map(|group| group.account.as_str())
            .collect()
    }

    /// Why this may be fine and why it may not be.
    ///
    /// Kept here rather than in a renderer so the human and JSON surfaces, and
    /// `fields auth`, `fields status`, and `sync`, all say the same thing.
    /// Deliberately separate from [`Self::remedy`] so a renderer can wrap this
    /// prose freely without ever breaking the command in that one across a line.
    #[must_use]
    pub fn advice() -> &'static str {
        "This is legitimate if you meant to collect a shared or delegated mailbox alongside your \
         own. If you did not, a browser reused an existing sign-in session during `fields auth`, \
         and one of these Fields is authenticated as the wrong person."
    }

    /// What to do about it, in one line, naming the command.
    #[must_use]
    pub fn remedy() -> &'static str {
        "Sign that account out, then run `fieldnotes fields auth <field_id>` again."
    }
}

/// Groups `recorded` by account, returning a mismatch only when the accounts
/// disagree.
///
/// The pure core of this module: `recorded` is `(field_id, account)` pairs for
/// Fields that have a recorded account, and a Field whose account is unknown
/// contributes nothing — an unknown account is not evidence of agreement *or*
/// of disagreement, so it must not be able to raise or suppress this warning.
#[must_use]
pub fn mismatch_of(recorded: &[(String, String)]) -> Option<AccountMismatch> {
    let mut grouped: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (field_id, account) in recorded {
        grouped
            .entry(account.as_str())
            .or_default()
            .push(field_id.clone());
    }
    if grouped.len() < 2 {
        return None;
    }
    Some(AccountMismatch {
        accounts: grouped
            .into_iter()
            .map(|(account, mut field_ids)| {
                field_ids.sort();
                AccountGroup {
                    account: account.to_owned(),
                    field_ids,
                }
            })
            .collect(),
    })
}

/// Reads every configured Field's recorded account and reports a disagreement.
///
/// Deliberately reads **every** configured Field, not just the ones a command
/// happens to be acting on: `fields status outlook_mail_work` and
/// `sync outlook_mail_work` should both still say that this notebook's Fields
/// are signed in as different people, because that is true of the notebook
/// whichever Field you asked about.
///
/// Disabled Fields are included too. A disabled Field's stored credential still
/// authenticates as somebody, and its Notes are still in the notebook.
pub fn account_mismatch(notebook: &Notebook) -> Result<Option<AccountMismatch>, AppError> {
    let recorded: Vec<(String, String)> = list_field_configs(notebook)?
        .into_iter()
        .filter_map(|config| {
            config
                .credential_account
                .map(|account| (config.id, account))
        })
        .collect();
    Ok(mismatch_of(&recorded))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(field, account)| ((*field).to_owned(), (*account).to_owned()))
            .collect()
    }

    #[test]
    fn two_fields_with_the_same_account_produce_no_warning() {
        assert_eq!(
            mismatch_of(&pairs(&[
                ("outlook_mail_work", "owner@example.test"),
                ("outlook_calendar_work", "owner@example.test"),
            ])),
            None
        );
    }

    #[test]
    fn two_fields_with_differing_accounts_name_both_accounts_and_both_fields() {
        let mismatch = mismatch_of(&pairs(&[
            ("outlook_mail_work", "owner@example.test"),
            ("outlook_contacts_work", "admin@example.test"),
            ("outlook_calendar_work", "owner@example.test"),
        ]))
        .unwrap_or_else(|| panic!("differing accounts must be reported"));
        // Ascending account order, so output is deterministic.
        assert_eq!(
            mismatch.account_names(),
            vec!["admin@example.test", "owner@example.test"]
        );
        assert_eq!(
            mismatch.accounts[0].field_ids,
            vec!["outlook_contacts_work".to_owned()]
        );
        assert_eq!(
            mismatch.accounts[1].field_ids,
            vec![
                "outlook_calendar_work".to_owned(),
                "outlook_mail_work".to_owned()
            ]
        );
        assert!(AccountMismatch::remedy().contains("fieldnotes fields auth"));
    }

    #[test]
    fn one_recorded_account_or_none_at_all_is_never_a_mismatch() {
        assert_eq!(mismatch_of(&[]), None);
        assert_eq!(
            mismatch_of(&pairs(&[("outlook_mail_work", "owner@example.test")])),
            None
        );
    }

    #[test]
    fn an_unknown_account_neither_raises_nor_suppresses_the_warning() {
        // Two Fields agree and a third has recorded nothing: still no warning,
        // because an unknown account is not a second account.
        assert_eq!(
            mismatch_of(&pairs(&[
                ("outlook_mail_work", "owner@example.test"),
                ("outlook_calendar_work", "owner@example.test"),
            ])),
            None
        );
        // And a disagreement between two known accounts is still reported when a
        // third Field's account is unknown, since the unknown one contributes no
        // pair at all.
        let mismatch = mismatch_of(&pairs(&[
            ("outlook_mail_work", "owner@example.test"),
            ("outlook_contacts_work", "admin@example.test"),
        ]))
        .unwrap_or_else(|| panic!("a disagreement must survive an unknown third Field"));
        assert_eq!(mismatch.accounts.len(), 2);
    }
}
