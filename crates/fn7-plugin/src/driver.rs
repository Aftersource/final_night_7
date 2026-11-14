//! Drive a whole match through the `GamePlugin` trait exactly as the Theatre core does (Start → Brief → Compose →
//! Resolve → Commit until `is_match_over`, then `summarize`), in-process. Used by `fn7-sim` and the tests.

use serde_json::Value;
use theatre_plugin_api::*;

/// One beat as the core saw it.
pub struct BeatReport {
    pub round: u32,
    pub start: RoundStart,
    pub outcome: RoundOutcome,
    pub committed: Committed,
    /// The actions the core passed to `resolve_round`.
    pub actions: Vec<Action>,
}

/// Actions for seats a script controls (`None` = the core's behavior: `bot_action` for bots, `default_action` else).
pub type Script<'a> = dyn Fn(Seat, u32) -> Option<Action> + Send + Sync + 'a;

/// Windowed and instant actions the core would deliver after this round's Compose actions (they must carry the
/// core's metadata: `windowed` / `window_id` / `instant`, `seq`, `received_at_ms`), and the `RoundContext` it would
/// hand the resolve (the action windows closed since the last one). Invalid actions are dropped, as the core rejects
/// them on arrival.
pub type Extras<'a> = dyn Fn(u32) -> (Vec<Action>, RoundContext) + Send + Sync + 'a;

/// Mid-match joins the core completes at a round's boundary (before its `on_round_start`), as `on_seat_takeover`
/// calls. A taken-over seat (`new_seat: false`) must be one of the seats; a new seat joins the actors from that round.
pub type Joins<'a> = dyn Fn(u32) -> Vec<SeatTakeover> + Send + Sync + 'a;

/// How the core would have received an action (for `validate_action`).
pub fn kind_of(a: &Action) -> SubmitKind {
    match (a.instant, a.window_id) {
        (true, _) => SubmitKind::Instant,
        (false, Some(window_id)) if a.windowed => SubmitKind::Window { window_id },
        _ => SubmitKind::Compose,
    }
}

/// `on_beat` runs after each commit with the committed state; returning false stops the match there (it is still
/// summarized).
pub async fn run_match(
    p: &dyn GamePlugin,
    seats: &[SeatInfo],
    settings: &Value,
    llm: &dyn LlmProvider,
    stop_after: Option<u32>,
    script: &Script<'_>,
    on_beat: impl FnMut(&BeatReport, &Value) -> bool,
) -> anyhow::Result<(Value, Summary)> {
    run_match_with(p, seats, settings, llm, stop_after, script, &|_| (vec![], RoundContext::default()), on_beat).await
}

/// [`run_match`] plus real-time input: `extras(round)`'s actions are appended to each round's actions, as the core
/// appends the windowed and instant actions delivered since the last resolve, and its context goes to the resolve.
#[allow(clippy::too_many_arguments)]
pub async fn run_match_with(
    p: &dyn GamePlugin,
    seats: &[SeatInfo],
    settings: &Value,
    llm: &dyn LlmProvider,
    stop_after: Option<u32>,
    script: &Script<'_>,
    extras: &Extras<'_>,
    on_beat: impl FnMut(&BeatReport, &Value) -> bool,
) -> anyhow::Result<(Value, Summary)> {
    run_match_joins(p, seats, settings, llm, stop_after, script, extras, &|_| vec![], on_beat).await
}

/// [`run_match_with`] plus mid-match joins: `joins(round)`'s takeovers go to `on_seat_takeover` at the round's
/// boundary, before `on_round_start`, as the core's `complete_joins` does; the joiner then acts as a human seat
/// (`script`, else `default_action`).
#[allow(clippy::too_many_arguments)]
pub async fn run_match_joins(
    p: &dyn GamePlugin,
    seats: &[SeatInfo],
    settings: &Value,
    llm: &dyn LlmProvider,
    stop_after: Option<u32>,
    script: &Script<'_>,
    extras: &Extras<'_>,
    joins: &Joins<'_>,
    mut on_beat: impl FnMut(&BeatReport, &Value) -> bool,
) -> anyhow::Result<(Value, Summary)> {
    let settings = p.validate_settings(settings).map_err(anyhow::Error::msg)?;
    let mut state = p.on_match_start(seats, &settings, llm).await?;
    let mut seats: Vec<SeatInfo> = seats.to_vec();
    let mut out: Vec<Seat> = vec![];
    let mut round = 0;
    loop {
        round += 1;
        for t in joins(round) {
            match seats.iter_mut().find(|s| s.seat == t.seat) {
                Some(s) => {
                    s.is_bot = false;
                    s.display_name = t.display_name.clone();
                }
                None => seats.push(SeatInfo { seat: t.seat, display_name: t.display_name.clone(), is_bot: false }),
            }
            out.retain(|s| *s != t.seat);
            if let Some(st) = p.on_seat_takeover(&state, &t) {
                state = st;
            }
        }
        let start = p.on_round_start(&state, round, llm).await?;
        state = start.state.clone();
        out.extend(start.eliminated.iter().map(|e| e.0));
        if p.is_match_over(&state, round - 1) {
            break;
        }
        let _brief = p.brief(&state, round, llm).await?;
        let mut actions = vec![];
        for s in seats.iter().filter(|s| !out.contains(&s.seat)) {
            let a = match script(s.seat, round) {
                Some(a) => a,
                None if s.is_bot => p.bot_action(&state, s.seat, llm).await?,
                None => p.default_action(&state, s.seat),
            };
            match p.validate_action(&state, s.seat, &a.text, &a.fields, SubmitKind::Compose) {
                Ok(()) => actions.push(a),
                Err(_) => actions.push(p.default_action(&state, s.seat)),
            }
        }
        let (mut late, ctx) = extras(round);
        late.retain(|a| {
            !out.contains(&a.seat) && p.validate_action(&state, a.seat, &a.text, &a.fields, kind_of(a)).is_ok()
        });
        late.sort_by_key(|a| a.seq);
        actions.extend(late);
        let outcome = match p
            .resolve_round(&state, round, &actions, &ctx, llm, &NoProgress)
            .await
        {
            Ok(o) => o,
            Err(_) => p.fallback_outcome(&state, round, &actions, &ctx),
        };
        state = outcome.state.clone();
        let committed = p.commit_round(&state, round, &[], llm).await?;
        state = committed.state.clone();
        out.extend(committed.eliminated.iter().map(|e| e.0));
        let rep = BeatReport {
            round,
            start,
            outcome,
            committed,
            actions,
        };
        let go_on = on_beat(&rep, &state);
        if !go_on || stop_after.is_some_and(|n| round >= n) || p.is_match_over(&state, round) {
            break;
        }
    }
    let summary = p.summarize(&state, llm).await?;
    Ok((state, summary))
}
