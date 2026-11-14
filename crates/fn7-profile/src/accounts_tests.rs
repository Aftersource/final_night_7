//! Accounts: the store's rules (names, sessions, Google, deletion, bindings) and what they do to history.

use rusqlite::Connection;
use serde_json::json;

use crate::accounts::*;
use crate::ingest::{read_recording_from, MatchInput, SeatIn, Source};
use crate::*;

fn store() -> ProfileStore {
    ProfileStore::open_in_memory().unwrap()
}

/// Make every session of `account` expired (a guest account lapses).
fn expire(s: &ProfileStore, account: &str) {
    s.lock().execute("UPDATE sessions SET expires_at = 1 WHERE account_id = ?1", [account]).unwrap();
}

fn code<T: std::fmt::Debug>(r: AResult<T>) -> &'static str {
    r.unwrap_err().code()
}

fn a_match(id: &str, at: i64, seats: Vec<SeatIn>) -> MatchInput {
    let standings: Vec<serde_json::Value> = seats
        .iter()
        .enumerate()
        .map(|(i, s)| json!({ "alive": i == 0, "demigod": format!("dg:{}", s.seat), "name": s.display_name, "rank": i + 1, "score": 1.0, "seat": s.seat }))
        .collect();
    MatchInput {
        match_id: id.into(),
        room_code: Some("ROOM01".into()),
        started_at: Some(at - 60_000),
        ended_at: Some(at),
        rules_version: Some("0.8.1".into()),
        beats_total: Some(30),
        forked_from: None,
        seats,
        summary: Some(json!({ "champion_seat": 0, "champions": [], "final_cut": { "title": "t" }, "standings": standings,
                              "piles": [], "storyboard": [], "stats": { "beats": 3, "beats_total": 30 } })),
        rounds: Some(3),
        public_state: None,
        eliminations: vec![],
        recording: None,
        source: Source::Live,
    }
}

fn seat(n: u8, name: &str, account: Option<&str>) -> SeatIn {
    SeatIn { seat: n, display_name: name.into(), is_bot: false, account: account.map(str::to_string) }
}

#[test]
fn migration_keeps_existing_profiles() {
    let dir = std::env::temp_dir().join(format!("fn7-accounts-mig-{}-{}", std::process::id(), now_ms()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.sqlite");
    {
        // A store as shipped before accounts: schema version 1 only.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(schema::MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1u32).unwrap();
        conn.execute(
            "INSERT INTO profiles (key, display_name, skeleton, description_user, created_at, updated_at)
             VALUES ('chris', 'Chris', 'chrls', 'since before accounts', 5, 5)",
            [],
        )
        .unwrap();
    }
    let s = ProfileStore::open(&path).unwrap();
    assert_eq!(s.version().unwrap(), 2);
    let p = s.profile("Chris").unwrap().expect("the profile survived");
    assert_eq!(p.description.user, "since before accounts");
    // And an account can take the name: the old profile becomes its linked profile.
    let (a, _) = s.create_guest("Chris", "test", None).unwrap();
    assert_eq!(a.profile, "chris");
    assert_eq!(s.profile("chris").unwrap().unwrap().description.user, "since before accounts");
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn guest_accounts_sessions_and_tokens() {
    let s = store();
    let (a, ses) = s.create_guest("  Chris  ", "Chris's PC\u{7}", None).unwrap();
    assert!(a.id.starts_with("acc_") && a.id.len() == 28);
    assert_eq!((a.display_name.as_str(), a.profile.as_str(), a.guest), ("Chris", "chris", true));
    assert_eq!(a.identities.len(), 1);
    assert_eq!(a.identities[0].provider, "guest");
    assert!(ses.token.starts_with("fn7s_") && ses.token.len() == 69, "an opaque random token");
    assert_eq!(ses.info.device_label, "Chris's PC");
    // The token is stored only as a hash.
    let stored: String = s.lock().query_row("SELECT token_hash FROM sessions", [], |r| r.get(0)).unwrap();
    assert_eq!(stored, token_hash(&ses.token));
    assert_ne!(stored, ses.token);
    let dump: String = s
        .lock()
        .query_row("SELECT group_concat(id || token_hash || device_label) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert!(!dump.contains(&ses.token));

    let (again, info) = s.session_account(&ses.token).unwrap().expect("the token authenticates");
    assert_eq!(again.id, a.id);
    assert_eq!(info.id, ses.info.id);
    assert!(s.session_account("fn7s_wrong").unwrap().is_none());
    assert!(s.session_account("").unwrap().is_none());

    // A second session, the list, logout of one, logout of all.
    let second = s.new_session(&a.id, "laptop").unwrap();
    let list = s.sessions(&a.id, Some(&ses.info.id)).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list.iter().filter(|x| x.current).count(), 1);
    assert!(s.revoke_session(&ses.info.id).unwrap());
    assert!(!s.revoke_session(&ses.info.id).unwrap(), "already revoked");
    assert!(s.session_account(&ses.token).unwrap().is_none(), "a revoked session is refused");
    assert!(s.session_account(&second.token).unwrap().is_some());
    assert_eq!(s.revoke_all_sessions(&a.id).unwrap(), 1);
    assert!(s.session_account(&second.token).unwrap().is_none());
}

#[test]
fn sessions_expire_and_slide() {
    let s = store();
    let (a, ses) = s.create_guest("Chris", "", None).unwrap();
    // Last used an hour ago, expiring in a minute: a use slides it to the full lifetime.
    let now = now_ms();
    s.lock()
        .execute("UPDATE sessions SET last_seen_at = ?1, expires_at = ?2", rusqlite::params![now - 3_600_000, now + 60_000])
        .unwrap();
    let (_, info) = s.session_account(&ses.token).unwrap().unwrap();
    assert!(info.expires_at >= now + SESSION_TTL_MS - 1000);
    expire(&s, &a.id);
    assert!(s.session_account(&ses.token).unwrap().is_none(), "an expired session is refused");
    assert!(s.sessions(&a.id, None).unwrap().is_empty());
}

#[test]
fn names_are_held_by_active_accounts() {
    let s = store();
    let (chris, _) = s.create_guest("Chris", "", None).unwrap();
    assert_eq!(code(s.create_guest("chris", "", None)), "name_taken");
    assert_eq!(code(s.create_guest("ＣＨＲＩＳ", "", None)), "name_taken", "NFKC folds fullwidth");
    assert_eq!(code(s.create_guest("Chr1s", "", None)), "name_lookalike");
    assert_eq!(code(s.create_guest("\u{200b}", "", None)), "bad_name");
    assert_eq!(code(s.create_guest("A name that is far too long to login", "", None)), "invalid");
    assert_eq!(s.name_holder("CHRIS").unwrap().unwrap().account_id, chris.id);
    assert!(s.name_holder("Benito").unwrap().is_none());

    // A guest whose last session lapsed gives the name up: the next guest takes it, and the name's profile with it.
    s.record_match(&a_match("m1", now_ms() + 1000, vec![seat(0, "Chris", Some(&chris.id))])).unwrap();
    expire(&s, &chris.id);
    assert!(s.name_holder("Chris").unwrap().is_none());
    let (next, _) = s.create_guest("Chris", "", None).unwrap();
    assert_ne!(next.id, chris.id);
    assert!(s.account(&chris.id).unwrap().is_none(), "the lapsed guest account is gone");
    assert_eq!(s.profile("Chris").unwrap().unwrap().totals.matches, 1, "the name's history stays with the name");
}

#[test]
fn a_claimed_profile_needs_its_secret() {
    let s = store();
    s.upsert_profile("Chris").unwrap();
    s.claim("Chris", "hunter22", None).unwrap();
    assert_eq!(code(s.create_guest("Chris", "", None)), "profile_claimed");
    assert_eq!(code(s.create_guest("Chris", "", Some("wrong-one"))), "profile_claimed");
    let (a, _) = s.create_guest("Chris", "", Some("hunter22")).unwrap();
    assert_eq!(a.profile, "chris");
    let how: String = s.lock().query_row("SELECT how FROM account_profiles WHERE profile_key = 'chris'", [], |r| r.get(0)).unwrap();
    assert_eq!(how, "secret");
    // The claim secret still works for what it protected.
    assert!(s.set_description("Chris", "mine", Some("hunter22")).is_ok());
    assert!(matches!(s.set_description("Chris", "not mine", None), Err(ProfileError::Locked)));

    // An unclaimed profile goes to the first account that takes the name.
    s.upsert_profile("Benito").unwrap();
    let (b, _) = s.create_guest("Benito", "", None).unwrap();
    let how: String = s.lock().query_row("SELECT how FROM account_profiles WHERE profile_key = 'benito'", [], |r| r.get(0)).unwrap();
    assert_eq!(how, "unclaimed");
    // A brand new name makes its profile.
    let (m, _) = s.create_guest("Mara", "", None).unwrap();
    assert!(s.profile("Mara").unwrap().is_some());
    assert_eq!(s.profile_account("mara").unwrap().unwrap().account_id, m.id);
    assert_eq!(s.profile_account("benito").unwrap().unwrap().account_id, b.id);
}

#[test]
fn renaming_keeps_the_old_name() {
    let s = store();
    let (a, _) = s.create_guest("Chris", "", None).unwrap();
    let (_b, _) = s.create_guest("Benito", "", None).unwrap();
    assert_eq!(code(s.rename_account(&a.id, "Benito", None)), "name_taken");
    assert_eq!(code(s.rename_account(&a.id, "Ben1to", None)), "name_lookalike");
    let r = s.rename_account(&a.id, "Kai", None).unwrap();
    assert_eq!((r.display_name.as_str(), r.profile.as_str()), ("Kai", "kai"));
    assert_eq!(r.past_profiles, vec!["chris".to_string()]);
    // The old name is still this account's: nobody else can take it, and it can go back.
    assert_eq!(code(s.create_guest("Chris", "", None)), "name_taken");
    assert_eq!(s.name_holder("Chris").unwrap().unwrap().account_id, a.id);
    let back = s.rename_account(&a.id, "chris", None).unwrap();
    assert_eq!(back.profile, "chris");
    assert_eq!(back.past_profiles, vec!["kai".to_string()]);
    // A respelling of the same name is not a new name.
    let since: i64 = s.lock().query_row("SELECT name_since FROM accounts WHERE id = ?1", [&a.id], |r| r.get(0)).unwrap();
    let again = s.rename_account(&a.id, "CHRIS", None).unwrap();
    assert_eq!(again.display_name, "CHRIS");
    let since2: i64 = s.lock().query_row("SELECT name_since FROM accounts WHERE id = ?1", [&a.id], |r| r.get(0)).unwrap();
    assert_eq!(since, since2);
}

#[test]
fn google_sign_in_creates_links_and_refuses_reuse() {
    let s = store();
    // First sight: a new account under the first free preferred name.
    let (taken, _) = s.create_guest("Chris", "", None).unwrap();
    let names = vec!["Chris".to_string(), "Chris D".to_string()];
    let (g, ses, created) = s.google_sign_in("sub-1", Some("  Chris@Example.com "), true, &names, "pc", None).unwrap();
    assert!(created && !g.guest);
    assert_eq!(g.display_name, "Chris 2", "the taken name gets a number before the next preference");
    assert_eq!(g.identities.len(), 1);
    assert_eq!(g.identities[0].email.as_deref(), Some("chris@example.com"));
    assert!(s.session_account(&ses.token).unwrap().is_some());
    // Again: the same account, a new session.
    let (g2, ses2, created) = s.google_sign_in("sub-1", Some("chris@example.com"), true, &names, "pc", None).unwrap();
    assert!(!created);
    assert_eq!(g2.id, g.id);
    assert_ne!(ses2.token, ses.token);

    // Linking Google to a guest keeps the guest's account (name, history), and it stops being a guest.
    let (linked, _, created) = s.google_sign_in("sub-2", Some("c2@example.com"), true, &[], "pc", Some(&taken.id)).unwrap();
    assert!(!created);
    assert_eq!(linked.id, taken.id);
    assert_eq!(linked.display_name, "Chris");
    assert!(!linked.guest);
    assert_eq!(linked.identities.iter().map(|i| i.provider.as_str()).collect::<Vec<_>>(), vec!["google"]);
    // A Google account stays active with no session at all: its name stays held.
    s.revoke_all_sessions(&taken.id).unwrap();
    assert_eq!(s.name_holder("Chris").unwrap().unwrap().account_id, taken.id);
    // And signing in with that Google identity (no link) finds it.
    let (found, _, created) = s.google_sign_in("sub-2", None, false, &[], "pc", None).unwrap();
    assert!(!created);
    assert_eq!(found.id, taken.id);

    // A Google identity that belongs to one account cannot be linked to another.
    let (other, _) = s.create_guest("Benito", "", None).unwrap();
    assert_eq!(code(s.google_sign_in("sub-1", None, true, &[], "pc", Some(&other.id))), "identity_in_use");
    // One Google identity per account.
    assert_eq!(code(s.google_sign_in("sub-3", None, true, &[], "pc", Some(&taken.id))), "invalid");
    // No usable name at all falls back to Player.
    let (p, _, _) = s.google_sign_in("sub-4", None, true, &["\u{200b}".to_string()], "pc", None).unwrap();
    assert_eq!(p.display_name, "Player");
    assert!(p.identities[0].email.is_none());
}

#[test]
fn emails_live_only_on_the_identity_row() {
    let s = store();
    let (g, _, _) = s.google_sign_in("sub-1", Some("secret.person@example.com"), true, &["Chris".into()], "pc", None).unwrap();
    s.record_match(&a_match("m1", now_ms() + 1000, vec![seat(0, "Chris", Some(&g.id))])).unwrap();
    let profile = serde_json::to_string(&s.profile("Chris").unwrap().unwrap()).unwrap();
    let history = serde_json::to_string(&s.history("Chris", 0, None).unwrap()).unwrap();
    assert!(!profile.contains("example.com") && !history.contains("example.com"));
    assert!(!profile.contains(&g.id), "the public profile doesn't name the account");
    // Every table but the identity row is free of it.
    let conn = s.lock();
    for (table, cols) in [
        ("accounts", "id || display_name || name_key"),
        ("sessions", "id || token_hash || device_label"),
        ("profiles", "key || display_name || description_user || description_auto"),
        ("match_players", "display_name || demigods || stats"),
    ] {
        let dump: Option<String> = conn.query_row(&format!("SELECT group_concat({cols}) FROM {table}"), [], |r| r.get(0)).unwrap();
        assert!(!dump.unwrap_or_default().contains("example.com"), "{table}");
    }
}

#[test]
fn history_accrues_only_to_bound_seats() {
    let s = store();
    let before = now_ms() - 10 * 60_000;
    // Before any account: the name's history, by name.
    s.record_match(&a_match("old", before, vec![seat(0, "Chris", None), seat(1, "Benito", None)])).unwrap();
    let (chris, _) = s.create_guest("Chris", "", None).unwrap();
    let later = now_ms() + 10 * 60_000;
    // An unbound seat under the account's name: left out. Benito has no account: recorded as always.
    let r = s.record_match(&a_match("impostor", later, vec![seat(0, "Chris", None), seat(1, "Benito", None)])).unwrap();
    assert_eq!(r.left_out, vec!["chris".to_string()]);
    assert_eq!(r.players, vec!["benito".to_string()]);
    // A seat bound to another account under the name: left out too.
    let r = s.record_match(&a_match("wrong", later + 1, vec![seat(0, "Chris", Some("acc_other"))])).unwrap();
    assert_eq!(r.left_out, vec!["chris".to_string()]);
    // The bound seat: recorded.
    let r = s.record_match(&a_match("mine", later + 2, vec![seat(0, "Chris", Some(&chris.id))])).unwrap();
    assert!(r.left_out.is_empty());
    let h = s.history("Chris", 0, None).unwrap();
    assert_eq!(h.items.iter().map(|m| m.match_id.as_str()).collect::<Vec<_>>(), vec!["mine", "old"]);
    assert_eq!(s.history("Benito", 0, None).unwrap().total, 2);
}

#[test]
fn a_recording_carries_the_binding() {
    let s = store();
    let (chris, _) = s.create_guest("Chris", "", None).unwrap();
    let at = now_ms() + 60_000;
    // The core writes `type` first (the reader classifies lines by their first bytes).
    let text = format!(
        concat!(
            r#"{{"type":"run_start","run":"rec1","plugin":"final_night_7","at":{at},"meta":{{"match_id":"rec1","code":"ABC123","#,
            r#""seats":[{{"seat":0,"display_name":"Chris","is_bot":false}},{{"seat":1,"display_name":"Chris","is_bot":false}}]}}}}"#,
            "\n",
            r#"{{"type":"event","id":4,"actor":"seat:0","ctx":{{"run":"rec1","round":0}},"kind":"seat_persona","at":{at},"#,
            r#""data":{{"round":0,"persona":{{"seat":0,"account":"{acc}","left":{{}},"right":{{}}}},"renamed":[]}}}}"#
        ),
        at = at,
        acc = chris.id
    );
    let (m, _) = read_recording_from(std::io::BufReader::new(text.as_bytes()), Some("rec1.jsonl".into())).unwrap();
    let m = m.unwrap();
    assert_eq!(m.seats[0].account.as_deref(), Some(chris.id.as_str()));
    assert_eq!(m.seats[1].account, None, "the other seat under the same name is not bound");
}

#[test]
fn deleting_an_account_removes_its_personal_data() {
    let s = store();
    let (g, ses, _) = s.google_sign_in("sub-1", Some("chris@example.com"), true, &["Chris".into()], "pc", None).unwrap();
    let at = now_ms() + 60_000;
    s.record_match(&a_match("m1", at, vec![seat(0, "Chris", Some(&g.id)), seat(1, "Benito", None)])).unwrap();
    s.add_image("Chris", &NewImage { hash: "h1".into(), kind: "portrait".into(), caption: "c".into(), subject: None,
                                      subject_name: None, match_id: Some("m1".into()), at: None }).unwrap();
    s.put_settings(&g.id, 1, &json!({ "version": 1 })).unwrap();
    let doc = DemigodDoc { name: Some("Ashen Vow".into()), personality: "dry".into(), ..Default::default() };
    s.put_demigod(&g.id, "left", &doc, &doc, "ok", "", &[], "rules").unwrap();
    s.grant_entitlement(&g.id, "founder", "active", "grant", None).unwrap();
    s.bind_player("player-1", &g.id, &ses.info.id).unwrap();
    s.set_description_owner("Chris", "about me").unwrap();

    let d = s.delete_account(&g.id).unwrap();
    assert_eq!((d.sessions, d.identities, d.bindings, d.history_rows, d.library_images), (1, 1, 1, 1, 1));
    assert_eq!(d.profiles, vec!["chris".to_string()]);
    assert!(s.account(&g.id).unwrap().is_none());
    assert!(s.session_account(&ses.token).unwrap().is_none());
    assert!(s.profile("Chris").unwrap().is_none());
    assert!(s.binding("player-1").unwrap().is_none());
    let conn = s.lock();
    for table in ["account_identities", "sessions", "account_settings", "account_demigods", "demigod_moderation",
                  "entitlements", "game_bindings", "account_profiles"] {
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "{table} is empty");
    }
    let rows: i64 = conn.query_row("SELECT COUNT(*) FROM match_players WHERE profile_key = 'chris'", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 0);
    // The other player's row and the match row itself stay.
    let rows: i64 = conn.query_row("SELECT COUNT(*) FROM match_players", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 1);
    // The tombstone holds a hash, not the name.
    let t: String = conn.query_row("SELECT key_hash FROM profile_tombstones", [], |r| r.get(0)).unwrap();
    assert!(!t.contains("chris") && t.len() == 64);
    drop(conn);

    // A backfill of the old match never rebuilds the deleted history; a match after the deletion is the name's again.
    let r = s.record_match(&a_match("m1", at, vec![seat(0, "Chris", Some(&g.id)), seat(1, "Benito", None)])).unwrap();
    assert_eq!(r.left_out, vec!["chris".to_string()]);
    assert!(s.profile("Chris").unwrap().is_none());
    let r = s.record_match(&a_match("m2", now_ms() + 3_600_000, vec![seat(0, "Chris", None)])).unwrap();
    assert!(r.left_out.is_empty());
    assert_eq!(code(s.delete_account(&g.id)), "not_found");
}

#[test]
fn settings_demigods_entitlements_round_trip() {
    let s = store();
    let (a, _) = s.create_guest("Chris", "", None).unwrap();
    let (b, _) = s.create_guest("Benito", "", None).unwrap();
    assert!(s.settings_doc(&a.id).unwrap().is_none());
    s.put_settings(&a.id, 1, &json!({ "version": 1, "pronouns": "they" })).unwrap();
    s.put_settings(&a.id, 1, &json!({ "version": 1, "pronouns": "she" })).unwrap();
    assert_eq!(s.settings_doc(&a.id).unwrap().unwrap().1["pronouns"], "she");

    assert!(s.demigod(&a.id, "left").unwrap().is_none());
    let original = DemigodDoc { name: Some("Ashen Vow".into()), personality: "I am  dry.\nVery dry.".into(), style: "terse".into(), creed: "debts".into() };
    let row = s.put_demigod(&a.id, "left", &original, &original, "ok", "", &[], "claude-haiku-5-5").unwrap();
    assert_eq!(row.r#final, original, "stored character for character");
    assert_eq!((row.verdict.as_str(), row.model.as_str()), ("ok", "claude-haiku-5-5"));
    let rewritten = DemigodDoc { personality: "Dry.".into(), ..original.clone() };
    let row = s.put_demigod(&a.id, "left", &original, &rewritten, "rephrase", "why", &["hate".into()], "rules").unwrap();
    assert_eq!((row.original.personality.as_str(), row.r#final.personality.as_str()), ("I am  dry.\nVery dry.", "Dry."));
    assert_eq!(code(s.put_demigod(&a.id, "middle", &original, &original, "ok", "", &[], "rules")), "invalid");
    s.log_refusal(&a.id, "right", &original, "nothing to keep", &["sexual".into()], "rules").unwrap();
    let log = s.moderation_log(&a.id).unwrap();
    assert_eq!(log.iter().map(|l| l["verdict"].as_str().unwrap()).collect::<Vec<_>>(), vec!["refuse", "rephrase", "ok"]);
    assert!(log[0]["final"].is_null());
    assert!(s.demigod(&a.id, "right").unwrap().is_none(), "a refusal stores no demigod");

    // Names: the other hand, another player's demigod, another player's name.
    assert!(s.demigod_name_conflict(&a.id, "right", "ashen vow").unwrap().unwrap().contains("two demigods"));
    assert!(s.demigod_name_conflict(&a.id, "left", "Ashen Vow").unwrap().is_none(), "its own name is fine");
    assert!(s.demigod_name_conflict(&b.id, "left", "A5hen V0w").unwrap().unwrap().contains("another player's demigod"));
    assert!(s.demigod_name_conflict(&b.id, "left", "Chr1s").unwrap().unwrap().contains("another player's name"));
    assert!(s.demigod_name_conflict(&b.id, "left", "Benito").unwrap().is_none(), "its own player name is fine");
    assert!(s.demigod_name_conflict(&b.id, "left", "\u{200b}").unwrap().is_some());

    // A lapsed guest's names are free again.
    let (c, _) = s.create_guest("Cyra", "", None).unwrap();
    let doc = DemigodDoc { name: Some("Cinder Oath".into()), ..Default::default() };
    s.put_demigod(&c.id, "left", &doc, &doc, "ok", "", &[], "rules").unwrap();
    assert!(s.demigod_name_conflict(&b.id, "left", "Cinder Oath").unwrap().is_some());
    assert!(s.demigod_name_conflict(&b.id, "left", "Cyra").unwrap().is_some());
    expire(&s, &c.id);
    assert!(s.demigod_name_conflict(&b.id, "left", "Cinder Oath").unwrap().is_none());
    assert!(s.demigod_name_conflict(&b.id, "left", "Cyra").unwrap().is_none());

    assert!(s.entitlements(&a.id).unwrap().is_empty(), "no rows: the free tier");
    s.grant_entitlement(&a.id, "founder", "active", "grant", Some(99)).unwrap();
    let e = s.entitlements(&a.id).unwrap();
    assert_eq!((e[0].sku.as_str(), e[0].status.as_str(), e[0].expires_at), ("founder", "active", Some(99)));

    // The moderation log is capped.
    for _ in 0..60 {
        s.log_refusal(&a.id, "right", &original, "x", &[], "rules").unwrap();
    }
    assert_eq!(s.moderation_log(&a.id).unwrap().len(), MODERATION_LOG_KEEP as usize);
}

#[test]
fn bindings_follow_their_session() {
    let s = store();
    let (a, ses) = s.create_guest("Chris", "", None).unwrap();
    let other = s.new_session(&a.id, "laptop").unwrap();
    s.bind_player("p1", &a.id, &ses.info.id).unwrap();
    s.bind_player("p2", &a.id, &other.info.id).unwrap();
    assert_eq!(s.binding("p1").unwrap().as_deref(), Some(a.id.as_str()));
    assert!(s.binding("nobody").unwrap().is_none());
    assert_eq!(s.prune_bindings(&["p1".into(), "p2".into()]).unwrap(), 0);
    // Logging one session out unbinds what it bound; the other stays.
    s.revoke_session(&ses.info.id).unwrap();
    assert!(s.binding("p1").unwrap().is_none());
    assert!(s.binding("p2").unwrap().is_some());
    assert_eq!(s.prune_bindings(&[]).unwrap(), 1);
}

#[cfg(feature = "http")]
mod http {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use crate::http::{router, HttpConfig};
    use crate::ProfileStore;

    async fn call(app: &axum::Router, method: &str, uri: &str, bearer: Option<&str>, body: Value) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri).header("content-type", "application/json");
        if let Some(t) = bearer {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let res = app.clone().oneshot(req.body(Body::from(body.to_string())).unwrap()).await.unwrap();
        let code = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (code, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn an_account_held_profile_is_written_only_by_its_account() {
        let store = Arc::new(ProfileStore::open_in_memory().unwrap());
        let (_, chris) = store.create_guest("Chris", "", None).unwrap();
        let (_, benito) = store.create_guest("Benito", "", None).unwrap();
        store.upsert_profile("Nobody").unwrap();
        let app = router(store.clone(), HttpConfig::default());
        let text = json!({ "text": "hello" });
        let (c, e) = call(&app, "PUT", "/api/profiles/chris/description", None, text.clone()).await;
        assert_eq!((c, e["error"].as_str()), (StatusCode::FORBIDDEN, Some("account_profile")));
        let (c, _) = call(&app, "PUT", "/api/profiles/chris/description", Some(&benito.token), text.clone()).await;
        assert_eq!(c, StatusCode::FORBIDDEN);
        let (c, p) = call(&app, "PUT", "/api/profiles/chris/description", Some(&chris.token), text.clone()).await;
        assert_eq!((c, p["description"]["user"].as_str()), (StatusCode::OK, Some("hello")));
        let (c, _) = call(&app, "POST", "/api/profiles/chris/claim", None, json!({ "secret": "hunter22" })).await;
        assert_eq!(c, StatusCode::FORBIDDEN);
        let (c, _) = call(&app, "POST", "/api/profiles/chris/claim", Some(&chris.token), json!({ "secret": "hunter22" })).await;
        assert_eq!(c, StatusCode::NO_CONTENT);
        // A name nobody holds works as before.
        let (c, _) = call(&app, "PUT", "/api/profiles/nobody/description", None, text).await;
        assert_eq!(c, StatusCode::OK);
    }
}
