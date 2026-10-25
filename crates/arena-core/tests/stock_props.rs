use arena_proto::{LotView, consume_plan, weighted_avg_cost};
use proptest::prelude::*;

fn reference(old_qty: i64, old_avg: i64, in_qty: i64, in_cost: i64) -> i64 {
    let total = (old_qty + in_qty) as u128;
    let sum = old_qty as u128 * old_avg as u128 + in_qty as u128 * in_cost as u128;
    let (q, r) = (sum / total, sum % total);
    (q + u128::from(2 * r >= total)) as i64
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn the_average_is_the_nearest_integer_to_the_exact_weighted_mean(
        oq in 0..=100_000_000i64,
        oa in 0..=1_000_000_000_000i64,
        iq in 1..=1_000_000i64,
        ic in 0..=1_000_000_000_000i64,
    ) {
        let got = weighted_avg_cost(oq, oa, iq, ic);
        prop_assert_eq!(got, reference(oq, oa, iq, ic));
        let total = i128::from(oq) + i128::from(iq);
        let sum = i128::from(oq) * i128::from(oa) + i128::from(iq) * i128::from(ic);
        prop_assert!((2 * (i128::from(got) * total - sum)).abs() <= total);
        let (lo, hi) = if oq == 0 { (ic, ic) } else { (oa.min(ic), oa.max(ic)) };
        prop_assert!((lo..=hi).contains(&got));
        prop_assert_eq!(got, weighted_avg_cost(iq, ic, oq, oa));
    }

    #[test]
    fn a_dearer_lot_never_lowers_the_average(
        oq in 0..=1_000_000i64,
        oa in 0..=1_000_000i64,
        iq in 1..=1_000_000i64,
        ic in 0..=1_000_000i64,
        more in 0..=1_000i64,
    ) {
        prop_assert!(weighted_avg_cost(oq, oa, iq, ic + more) >= weighted_avg_cost(oq, oa, iq, ic));
        prop_assert!(weighted_avg_cost(oq, oa + more, iq, ic) >= weighted_avg_cost(oq, oa, iq, ic));
    }

    #[test]
    fn equal_costs_keep_the_cost(oq in 1..=100_000_000i64, iq in 1..=1_000_000i64, c in 0..=1_000_000_000_000i64) {
        prop_assert_eq!(weighted_avg_cost(oq, c, iq, c), c);
    }

    #[test]
    fn a_plan_takes_exactly_the_quantity_earliest_expiry_first(
        raw in prop::collection::vec(
            (prop::option::of(0..4u8), 0..4i64, 0..20i64),
            0..8,
        ),
        qty in -2..=60i64,
    ) {
        let lots: Vec<LotView> = raw
            .iter()
            .enumerate()
            .map(|(i, (e, at, rem))| LotView {
                lot_id: format!("L{i}"),
                expiry: e.map(|d| format!("2027-01-0{}", d + 1)),
                received_at: *at,
                remaining: *rem,
            })
            .collect();
        let total: i64 = lots.iter().map(|l| l.remaining).sum();
        let plan = consume_plan(&lots, qty);
        prop_assert_eq!(plan.is_some(), qty > 0 && qty <= total);
        let Some(plan) = plan else { return Ok(()) };
        prop_assert_eq!(plan.iter().map(|(_, n)| n).sum::<i64>(), qty);
        let key = |l: &LotView| (l.expiry.is_none(), l.expiry.clone(), l.received_at, l.lot_id.clone());
        let taken = |l: &LotView| plan.iter().find(|(id, _)| *id == l.lot_id).map_or(0, |p| p.1);
        for l in &lots {
            prop_assert!(taken(l) <= l.remaining);
            prop_assert!(taken(l) == 0 || l.remaining > 0);
            if taken(l) > 0 && taken(l) < l.remaining {
                for o in lots.iter().filter(|o| o.remaining > 0 && key(o) > key(l)) {
                    prop_assert_eq!(taken(o), 0);
                }
            }
            if taken(l) == 0 && l.remaining > 0 {
                for o in lots.iter().filter(|o| key(o) > key(l)) {
                    prop_assert_eq!(taken(o), 0);
                }
            }
        }
    }
}
