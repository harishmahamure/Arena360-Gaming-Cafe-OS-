use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub rate_bp: u16,
    pub amount_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RateTax {
    pub rate_bp: u16,
    pub taxable_minor: i64,
    pub tax_minor: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cgst_minor: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sgst_minor: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bill {
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
    pub breakup: Vec<RateTax>,
}

fn round_div(n: i128, d: i128) -> i128 {
    n.signum() * ((2 * n.abs() + d) / (2 * d))
}

pub fn compute(lines: &[Line], inclusive: bool, gst: bool) -> Option<Bill> {
    let mut per_rate = BTreeMap::<u16, i128>::new();
    for l in lines {
        *per_rate.entry(l.rate_bp).or_default() += i128::from(l.amount_minor);
    }
    let (mut subtotal, mut tax_sum, mut breakup) = (0i128, 0i128, Vec::new());
    for (rate_bp, amount) in per_rate {
        let r = i128::from(rate_bp);
        let tax = if inclusive {
            round_div(amount * r, 10_000 + r)
        } else {
            round_div(amount * r, 10_000)
        };
        let taxable = if inclusive { amount - tax } else { amount };
        let cgst = gst.then(|| round_div(tax, 2));
        subtotal += taxable;
        tax_sum += tax;
        breakup.push(RateTax {
            rate_bp,
            taxable_minor: taxable.try_into().ok()?,
            tax_minor: tax.try_into().ok()?,
            cgst_minor: cgst.map(i64::try_from).transpose().ok()?,
            sgst_minor: cgst.map(|c| i64::try_from(tax - c)).transpose().ok()?,
        });
    }
    Some(Bill {
        subtotal_minor: subtotal.try_into().ok()?,
        tax_minor: tax_sum.try_into().ok()?,
        total_minor: (subtotal + tax_sum).try_into().ok()?,
        breakup,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn l(rate_bp: u16, amount_minor: i64) -> Line {
        Line {
            rate_bp,
            amount_minor,
        }
    }

    #[test]
    fn golden_inclusive_five_percent() {
        let b = compute(&[l(500, 31_000)], true, true).unwrap();
        assert_eq!(
            (b.subtotal_minor, b.tax_minor, b.total_minor),
            (29_524, 1_476, 31_000)
        );
        let r = &b.breakup[0];
        assert_eq!(
            (r.taxable_minor, r.tax_minor, r.cgst_minor, r.sgst_minor),
            (29_524, 1_476, Some(738), Some(738))
        );
    }

    #[test]
    fn simple_mode_has_no_split() {
        let b = compute(&[l(500, 31_000)], true, false).unwrap();
        assert_eq!(b.breakup[0].cgst_minor, None);
        assert_eq!(
            serde_json::to_string(&b.breakup).unwrap(),
            r#"[{"rate_bp":500,"taxable_minor":29524,"tax_minor":1476}]"#
        );
    }

    #[test]
    fn exclusive_adds_tax_after_subtotal() {
        let b = compute(&[l(500, 31_000)], false, true).unwrap();
        assert_eq!(
            (b.subtotal_minor, b.tax_minor, b.total_minor),
            (31_000, 1_550, 32_550)
        );
        assert_eq!(b.breakup[0].taxable_minor, 31_000);
    }

    #[test]
    fn odd_tax_splits_cgst_up_and_sgst_down() {
        let b = compute(&[l(500, 1_000)], false, true).unwrap();
        let r = &b.breakup[0];
        assert_eq!(
            (r.tax_minor, r.cgst_minor, r.sgst_minor),
            (50, Some(25), Some(25))
        );
        let b = compute(&[l(100, 1_050)], false, true).unwrap();
        let r = &b.breakup[0];
        assert_eq!(
            (r.tax_minor, r.cgst_minor, r.sgst_minor),
            (11, Some(6), Some(5))
        );
    }

    #[test]
    fn rounds_once_per_rate_never_per_line() {
        let lines = [
            l(500, 110),
            l(500, 110),
            l(500, 110),
            l(1800, 100),
            l(1800, 100),
        ];
        let b = compute(&lines, true, true).unwrap();
        assert_eq!(b.breakup.len(), 2);
        assert_eq!((b.breakup[0].rate_bp, b.breakup[0].tax_minor), (500, 16));
        assert_eq!((b.breakup[1].rate_bp, b.breakup[1].tax_minor), (1800, 31));
        let per_line: i64 = lines
            .iter()
            .map(|x| compute(&[*x], true, true).unwrap().tax_minor)
            .sum();
        assert_eq!(per_line, 15 + 30);
    }

    #[test]
    fn zero_and_three_exponent_currencies_round_at_their_minor_unit() {
        let jpy = compute(&[l(500, 310)], true, true).unwrap();
        assert_eq!(
            (jpy.tax_minor, jpy.subtotal_minor, jpy.total_minor),
            (15, 295, 310)
        );
        let kwd = compute(&[l(500, 310_000)], true, true).unwrap();
        assert_eq!(
            (kwd.tax_minor, kwd.subtotal_minor, kwd.total_minor),
            (14_762, 295_238, 310_000)
        );
    }

    #[test]
    fn half_boundaries_round_up() {
        for (amount, rate, tax) in [(10, 5_000, 5), (1, 5_000, 1), (3, 1_667, 1), (1, 4_999, 0)] {
            assert_eq!(
                compute(&[l(rate, amount)], false, false).unwrap().tax_minor,
                tax,
                "{amount} @ {rate}"
            );
        }
        assert_eq!(compute(&[l(1_000, 5)], false, false).unwrap().tax_minor, 1);
        assert_eq!(compute(&[l(1_000, 4)], false, false).unwrap().tax_minor, 0);
        assert_eq!(compute(&[l(500, 21)], true, false).unwrap().tax_minor, 1);
        assert_eq!(compute(&[l(500, 20)], true, false).unwrap().tax_minor, 1);
        assert_eq!(compute(&[l(500, 10)], true, false).unwrap().tax_minor, 0);
    }

    #[test]
    fn empty_zero_rate_and_overflow() {
        let e = compute(&[], true, true).unwrap();
        assert_eq!((e.total_minor, e.breakup.len()), (0, 0));
        let z = compute(&[l(0, 999)], true, true).unwrap();
        assert_eq!(
            (z.tax_minor, z.subtotal_minor, z.total_minor),
            (0, 999, 999)
        );
        assert_eq!(z.breakup[0].cgst_minor, Some(0));
        assert!(compute(&[l(500, i64::MAX), l(500, i64::MAX)], false, false).is_none());
        assert!(compute(&[l(500, i64::MAX)], false, false).is_none());
        assert!(compute(&[l(0, i64::MAX)], true, false).is_some());
    }

    #[test]
    fn negative_amounts_mirror_positive_ones() {
        let p = compute(&[l(500, 31_000), l(1800, 12_345)], true, true).unwrap();
        let n = compute(&[l(500, -31_000), l(1800, -12_345)], true, true).unwrap();
        assert_eq!(n.tax_minor, -p.tax_minor);
        assert_eq!(n.total_minor, -p.total_minor);
    }

    fn lines() -> impl Strategy<Value = Vec<Line>> {
        let rate = prop_oneof![
            prop::sample::select(vec![
                0u16, 100, 500, 1200, 1667, 1800, 2000, 2800, 4999, 10_000
            ]),
            any::<u16>()
        ];
        let amount = prop_oneof![
            -200i64..200,
            -10_000_000i64..10_000_000,
            -(i64::MAX / 200_000)..i64::MAX / 200_000
        ];
        prop::collection::vec(
            (rate, amount).prop_map(|(rate_bp, amount_minor)| Line {
                rate_bp,
                amount_minor,
            }),
            0..12,
        )
    }

    fn negated(b: &Bill) -> Bill {
        Bill {
            subtotal_minor: -b.subtotal_minor,
            tax_minor: -b.tax_minor,
            total_minor: -b.total_minor,
            breakup: b
                .breakup
                .iter()
                .map(|r| RateTax {
                    rate_bp: r.rate_bp,
                    taxable_minor: -r.taxable_minor,
                    tax_minor: -r.tax_minor,
                    cgst_minor: r.cgst_minor.map(|v| -v),
                    sgst_minor: r.sgst_minor.map(|v| -v),
                })
                .collect(),
        }
    }

    proptest! {
        #[test]
        fn totals_are_consistent(ls in lines(), inclusive: bool, gst: bool) {
            let b = compute(&ls, inclusive, gst).unwrap();
            let sum: i64 = ls.iter().map(|x| x.amount_minor).sum();
            prop_assert_eq!(b.tax_minor, b.breakup.iter().map(|r| r.tax_minor).sum::<i64>());
            prop_assert_eq!(b.subtotal_minor, b.breakup.iter().map(|r| r.taxable_minor).sum::<i64>());
            prop_assert_eq!(b.total_minor, b.subtotal_minor + b.tax_minor);
            prop_assert_eq!(b.total_minor, if inclusive { sum } else { sum + b.tax_minor });
            prop_assert!(b.breakup.windows(2).all(|w| w[0].rate_bp < w[1].rate_bp));
            for r in &b.breakup {
                let gross = if inclusive { r.taxable_minor + r.tax_minor } else { r.taxable_minor };
                let exact_num = i128::from(gross) * i128::from(r.rate_bp);
                let den = if inclusive { 10_000 + i128::from(r.rate_bp) } else { 10_000 };
                let scaled = i128::from(r.tax_minor) * den;
                prop_assert!((scaled - exact_num).abs() * 2 <= den);
                prop_assert!(scaled.abs() >= exact_num.abs() || (scaled - exact_num).abs() * 2 < den);
                match (r.cgst_minor, r.sgst_minor) {
                    (Some(c), Some(s)) => {
                        prop_assert!(gst);
                        prop_assert_eq!(c + s, r.tax_minor);
                        prop_assert!((c - s).abs() <= 1 && (c - s) * r.tax_minor.signum() >= 0);
                    }
                    (None, None) => prop_assert!(!gst),
                    _ => prop_assert!(false),
                }
            }
        }

        #[test]
        fn order_and_line_splitting_do_not_matter(ls in lines(), inclusive: bool) {
            let a = compute(&ls, inclusive, true).unwrap();
            let mut rev = ls.clone();
            rev.reverse();
            prop_assert_eq!(&a, &compute(&rev, inclusive, true).unwrap());
            let split: Vec<Line> = ls
                .iter()
                .flat_map(|x| {
                    let h = x.amount_minor / 2;
                    [l(x.rate_bp, h), l(x.rate_bp, x.amount_minor - h)]
                })
                .collect();
            prop_assert_eq!(a, compute(&split, inclusive, true).unwrap());
        }

        #[test]
        fn negation_mirrors_every_figure(ls in lines(), inclusive: bool, gst: bool) {
            let neg: Vec<Line> = ls.iter().map(|x| l(x.rate_bp, -x.amount_minor)).collect();
            let p = compute(&ls, inclusive, gst).unwrap();
            prop_assert_eq!(compute(&neg, inclusive, gst).unwrap(), negated(&p));
        }
    }

    #[test]
    fn negative_odd_tax_splits_larger_magnitude_to_cgst() {
        let b = compute(&[l(100, -1_050)], false, true).unwrap();
        let r = &b.breakup[0];
        assert_eq!(
            (r.tax_minor, r.cgst_minor, r.sgst_minor),
            (-11, Some(-6), Some(-5))
        );
    }

    #[test]
    fn ties_round_away_from_zero_inclusive_and_exclusive() {
        for (amount, rate, incl, tax) in [
            (3, 2_000, true, 1),
            (-3, 2_000, true, -1),
            (9, 2_000, true, 2),
            (-9, 2_000, true, -2),
            (-1, 5_000, false, -1),
            (-5, 1_000, false, -1),
        ] {
            assert_eq!(
                compute(&[l(rate, amount)], incl, false).unwrap().tax_minor,
                tax,
                "{amount} @ {rate} {incl}"
            );
        }
    }

    #[test]
    fn mixed_sign_lines_net_before_rounding() {
        let a = compute(&[l(500, 31_000), l(500, -10_000)], true, true).unwrap();
        assert_eq!(a, compute(&[l(500, 21_000)], true, true).unwrap());
    }

    #[test]
    fn zero_rate_group_is_kept_and_ordered_first() {
        let b = compute(&[l(500, 100), l(0, 100)], false, true).unwrap();
        assert_eq!(
            b.breakup.iter().map(|r| r.rate_bp).collect::<Vec<_>>(),
            [0, 500]
        );
        assert_eq!((b.tax_minor, b.total_minor), (5, 205));
    }

    #[test]
    fn overflow_in_every_figure_and_extremes() {
        assert!(compute(&[l(0, i64::MAX), l(10_000, i64::MAX)], true, true).is_none());
        assert!(
            compute(
                &[l(0, i64::MAX), l(0, i64::MAX), l(500, -i64::MAX)],
                false,
                false
            )
            .is_none()
        );
        assert!(compute(&[l(500, i64::MIN)], false, false).is_none());
        assert!(compute(&[l(0, i64::MIN), l(0, -1)], true, false).is_none());
        assert_eq!(
            compute(&[l(0, i64::MIN)], true, false).unwrap().total_minor,
            i64::MIN
        );
        let m = compute(&[l(10_000, i64::MAX)], true, true).unwrap();
        let r = &m.breakup[0];
        assert_eq!(r.tax_minor, i64::MAX / 2 + 1);
        assert_eq!(r.cgst_minor.unwrap() + r.sgst_minor.unwrap(), r.tax_minor);
        let x = compute(&[l(10_000, 101)], true, true).unwrap().breakup[0].clone();
        assert_eq!(
            (x.tax_minor, x.cgst_minor, x.sgst_minor),
            (51, Some(26), Some(25))
        );
        assert_eq!(
            compute(&[l(10_000, 100)], false, true).unwrap().total_minor,
            200
        );
    }
}
