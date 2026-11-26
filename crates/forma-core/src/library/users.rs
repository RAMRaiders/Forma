//! Who is playing (#301). The library's first user, the admin, owns the pool: every game in
//! the library. Each user has their own favourites, play history and list (the pool, less the
//! games they took off it), and their own accounts ([`Library::user_setting`]). Every game's
//! row is worked out for the user who entered ([`Library::enter`]), so lists, sorting and
//! search need nothing of their own.

use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{GAME_COLUMNS, Game, Library, materialise};
use crate::error::{Error, Result};
use crate::field;

/// The admin: the library's first user, who cannot be removed.
pub const ADMIN: i64 = 1;

/// Forma's setting naming the user who entered last: Forma starts with them.
pub const CURRENT_USER_SETTING: &str = "users.current";

/// What is each user's own: on the admin, a gamelist's value counts as theirs too (as before
/// users); another user starts without any.
pub(crate) const PER_USER: [&str; 4] = [
    field::FAVORITE,
    field::PLAY_COUNT,
    field::LAST_PLAYED,
    field::GAME_TIME,
];

/// The longest a user's name may be.
const NAME_MAX: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i64,
    pub name: String,
    /// The admin: may change the pool, Forma and its users.
    pub admin: bool,
    /// Whether entering as them asks for a PIN.
    pub pin: bool,
}

/// The user whose games are worked out now, and whether they are the admin.
pub(crate) fn current(conn: &rusqlite::Connection) -> Result<(i64, bool)> {
    let chosen: Option<i64> = conn
        .query_row(
            "SELECT u.id FROM setting s JOIN user u ON u.id = CAST(s.value AS INTEGER)
             WHERE s.key = ?1",
            [CURRENT_USER_SETTING],
            |r| r.get(0),
        )
        .optional()?;
    let id = chosen.unwrap_or(ADMIN);
    Ok((id, id == ADMIN))
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

/// `pin` salted and hashed as the `user` table keeps it.
fn hashed(salt: &str, pin: &str) -> String {
    let digest = Sha256::digest(format!("{salt}:{pin}").as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{salt}${hex}")
}

/// A salt that differs from PIN to PIN: what keeps two equal PINs from looking equal.
fn salt() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let digest = Sha256::digest(format!("{nanos}:{}", std::process::id()).as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

impl Library {
    /// Every user, the admin first, then by name.
    pub fn users(&self) -> Result<Vec<User>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name, admin, pin IS NOT NULL FROM user
             ORDER BY admin DESC, name COLLATE NOCASE",
        )?;
        let users = stmt
            .query_map([], |r| {
                Ok(User {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    admin: r.get(2)?,
                    pin: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(users)
    }

    /// The user who entered last (the admin until another does).
    pub fn current_user(&self) -> Result<User> {
        let (id, _) = current(&self.conn)?;
        self.users()?
            .into_iter()
            .find(|u| u.id == id)
            .ok_or_else(|| invalid("no admin user"))
    }

    /// A new user, their list the whole pool. Names are unique, whatever their case.
    pub fn add_user(&mut self, name: &str) -> Result<User> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > NAME_MAX {
            return Err(invalid(format!("a name is 1–{NAME_MAX} characters")));
        }
        if self
            .users()?
            .iter()
            .any(|u| u.name.eq_ignore_ascii_case(name))
        {
            return Err(invalid(format!("there is a user called {name} already")));
        }
        self.conn
            .execute("INSERT INTO user (name) VALUES (?1)", [name])?;
        let id = self.conn.last_insert_rowid();
        Ok(User {
            id,
            name: name.to_owned(),
            admin: false,
            pin: false,
        })
    }

    /// Removes a user, with their favourites, history, list and accounts. Not the admin. If
    /// they are the one in, the admin is.
    pub fn remove_user(&mut self, id: i64) -> Result<()> {
        if id == ADMIN {
            return Err(invalid("the admin cannot be removed"));
        }
        let was_in = current(&self.conn)?.0 == id;
        if self.conn.execute("DELETE FROM user WHERE id = ?1", [id])? == 0 {
            return Err(invalid(format!("no user {id}")));
        }
        if was_in {
            self.enter(ADMIN)?;
        }
        Ok(())
    }

    /// Sets (or with `None`, takes off) the admin's PIN: 4 to 8 digits.
    pub fn set_pin(&mut self, pin: Option<&str>) -> Result<()> {
        let stored = match pin {
            Some(pin) => {
                if !(4..=8).contains(&pin.len()) || !pin.chars().all(|c| c.is_ascii_digit()) {
                    return Err(invalid("a PIN is 4 to 8 digits"));
                }
                Some(hashed(&salt(), pin))
            }
            None => None,
        };
        self.conn.execute(
            "UPDATE user SET pin = ?2 WHERE id = ?1",
            params![ADMIN, stored],
        )?;
        Ok(())
    }

    /// Whether `pin` lets one enter as user `id`: any does for a user without one.
    pub fn pin_matches(&self, id: i64, pin: Option<&str>) -> Result<bool> {
        let stored: Option<Option<String>> = self
            .conn
            .query_row("SELECT pin FROM user WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        Ok(match stored {
            None => false,
            Some(None) => true,
            Some(Some(stored)) => {
                let salt = stored.split('$').next().unwrap_or_default();
                pin.is_some_and(|pin| hashed(salt, pin) == stored)
            }
        })
    }

    /// `id` is the one playing from now on: every game worked out again for them.
    pub fn enter(&mut self, id: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        let known: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM user WHERE id = ?1)",
            [id],
            |r| r.get(0),
        )?;
        if !known {
            return Err(invalid(format!("no user {id}")));
        }
        tx.execute(
            "INSERT INTO setting (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![CURRENT_USER_SETTING, id.to_string()],
        )?;
        let ids: Vec<i64> = tx
            .prepare("SELECT id FROM game")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        materialise(&tx, &ids, &[])?;
        tx.commit()?;
        Ok(())
    }

    /// Puts a game of the pool back on the current user's list, or takes it off.
    pub fn set_listed(&mut self, game: i64, listed: bool) -> Result<()> {
        if listed {
            self.set_user_field(game, field::REMOVED, None)
        } else {
            self.set_user_field(game, field::REMOVED, Some("1"))
        }
    }

    /// The pool's games of `system` the current user took off their list, by name.
    pub fn unlisted_games(&self, system: &str) -> Result<Vec<Game>> {
        let (user, _) = current(&self.conn)?;
        self.query_games(
            &format!(
                "SELECT {GAME_COLUMNS} FROM game g
                 JOIN user_field u ON u.game_id = g.id AND u.user_id = ?1
                     AND u.field = '{}' AND u.value = '1'
                 WHERE g.system = ?2 AND g.present = 1
                   AND NOT EXISTS (SELECT 1 FROM game_field f WHERE f.game_id = g.id
                                   AND f.field = '{}' AND f.value = '1')
                 ORDER BY g.name COLLATE NOCASE",
                field::REMOVED,
                field::HIDDEN
            ),
            params![user, system],
        )
    }

    /// One of user `user`'s own settings (an account's name, say), if set.
    pub fn user_setting(&self, user: i64, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM user_setting WHERE user_id = ?1 AND key = ?2",
                params![user, key],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Sets (or with `None`, clears) one of user `user`'s own settings.
    pub fn set_user_setting(&mut self, user: i64, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(value) => self.conn.execute(
                "INSERT INTO user_setting (user_id, key, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT (user_id, key) DO UPDATE SET value = excluded.value",
                params![user, key, value],
            )?,
            None => self.conn.execute(
                "DELETE FROM user_setting WHERE user_id = ?1 AND key = ?2",
                params![user, key],
            )?,
        };
        Ok(())
    }

    /// Sets (or with `None`, clears) one of the current user's own values for a game, then
    /// works the game out again.
    pub(crate) fn set_user_field(
        &mut self,
        game: i64,
        key: &str,
        value: Option<&str>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        let (user, _) = current(&tx)?;
        match value {
            Some(value) => tx.execute(
                "INSERT INTO user_field (user_id, game_id, field, value) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (user_id, game_id, field) DO UPDATE SET value = excluded.value",
                params![user, game, key, value],
            )?,
            None => tx.execute(
                "DELETE FROM user_field WHERE user_id = ?1 AND game_id = ?2 AND field = ?3",
                params![user, game, key],
            )?,
        };
        materialise(&tx, &[game], &[])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pin_is_kept_hashed_and_salted() {
        let a = hashed("aa", "1234");
        assert!(a.starts_with("aa$"));
        assert!(!a.contains("1234"));
        assert_ne!(a, hashed("bb", "1234"));
        assert_ne!(salt(), "");
    }
}
