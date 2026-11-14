//! The society Venn: membership queries and the group overlap graph (which also drives rumor spread).

use fn7_model::*;

/// Every member (alive demigods via their Following, clusters, alive characters) in all of `all_of` and none of
/// `none_of`. Order: demigods, clusters, characters, each by id.
pub fn members(w: &World, all_of: &[GroupId], none_of: &[GroupId]) -> Vec<Ref> {
    let ok = |gs: &GroupSet| {
        all_of.iter().all(|g| gs.contains(*g)) && !none_of.iter().any(|g| gs.contains(*g))
    };
    let mut out = vec![];
    out.extend(
        w.demigods
            .iter()
            .filter(|d| d.alive && ok(&GroupSet::new(vec![d.following])))
            .map(|d| Ref::Demigod(d.id)),
    );
    out.extend(
        w.clusters
            .iter()
            .filter(|c| ok(&c.groups))
            .map(|c| Ref::Cluster(c.id)),
    );
    out.extend(
        w.characters
            .iter()
            .filter(|c| c.alive && ok(&c.groups))
            .map(|c| Ref::Char(c.id)),
    );
    out
}

/// Head count of one member: a cluster's unmaterialized size, or 1 for a person.
fn heads(w: &World, r: Ref) -> u64 {
    match r {
        Ref::Cluster(c) => w.clusters[c.ix()].size as u64,
        _ => 1,
    }
}

/// People (head count) in a group.
pub fn headcount(w: &World, g: GroupId) -> u64 {
    members(w, &[g], &[]).into_iter().map(|r| heads(w, r)).sum()
}

/// Dense pairwise shared head counts (`n×n`, symmetric) and per-group totals.
pub(crate) fn shared_heads(w: &World) -> (usize, Vec<u64>, Vec<u64>) {
    let n = w.groups.len();
    let mut shared = vec![0u64; n * n];
    let mut total = vec![0u64; n];
    let mut add = |gs: &GroupSet, h: u64| {
        let s = gs.as_slice();
        for (i, a) in s.iter().enumerate() {
            if a.ix() >= n {
                continue;
            }
            total[a.ix()] += h;
            for b in &s[i + 1..] {
                if b.ix() < n {
                    shared[a.ix() * n + b.ix()] += h;
                    shared[b.ix() * n + a.ix()] += h;
                }
            }
        }
    };
    for c in &w.clusters {
        add(&c.groups, (c.size as u64).max(1));
    }
    for c in w.characters.iter().filter(|c| c.alive) {
        add(&c.groups, 1);
    }
    for d in w.demigods.iter().filter(|d| d.alive) {
        add(&GroupSet::new(vec![d.following]), 1);
    }
    (n, shared, total)
}

/// Overlap coefficient of every overlapping pair: shared heads / the smaller group's heads (0..1], `a < b`, sorted.
pub fn overlap_graph(w: &World) -> Vec<(GroupId, GroupId, f32)> {
    let (n, shared, total) = shared_heads(w);
    let mut out = vec![];
    for a in 0..n {
        for b in a + 1..n {
            let s = shared[a * n + b];
            if s > 0 {
                let m = total[a].min(total[b]).max(1);
                out.push((
                    GroupId::from_ix(a),
                    GroupId::from_ix(b),
                    (s as f64 / m as f64).min(1.0) as f32,
                ));
            }
        }
    }
    out
}
