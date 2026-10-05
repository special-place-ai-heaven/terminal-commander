// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Every `examples` entry in every built-in rule pack must hold against its
//! own rule: a `match` expectation fires (with the stated kind/captures) and
//! a `{"match": false}` expectation does not. Examples are otherwise never
//! evaluated at runtime, so this is the only place they are checked.

use terminal_commander_core::{
    BucketId, ProbeId, RuleExampleExpect, RuleStatus, SourceFrame, SourceStream,
};
use terminal_commander_sifters::SifterRuntime;
use terminal_commander_store::{RulePackFile, known_pack_names, resolve_pack_json};

#[test]
fn pack_examples_hold_against_their_own_rules() {
    let mut checked = 0;
    for pack in known_pack_names() {
        let parsed: RulePackFile =
            serde_json::from_str(resolve_pack_json(pack).expect("known pack"))
                .expect("pack parses");
        for mut rule in parsed.rules {
            rule.status = RuleStatus::Active;
            let sifter = SifterRuntime::build(std::slice::from_ref(&rule)).expect("rule builds");
            for ex in &rule.examples {
                let stream = ex
                    .stream
                    .clone()
                    .or_else(|| rule.stream.clone())
                    .unwrap_or(SourceStream::Stdout);
                let frame = SourceFrame::new(ProbeId::new(), stream, ex.input.clone());
                let drafts = sifter.evaluate(&frame, BucketId::new());
                let who = format!("{pack}/{} example {:?}", rule.id, ex.input);
                match &ex.expect {
                    RuleExampleExpect::NoMatch { .. } => {
                        assert!(
                            drafts.is_empty(),
                            "{who}: expected no match, got {drafts:?}"
                        );
                    }
                    RuleExampleExpect::Match { kind, captures } => {
                        assert!(!drafts.is_empty(), "{who}: expected a match, got none");
                        if let Some(kind) = kind {
                            assert_eq!(&drafts[0].kind, kind, "{who}: kind");
                        }
                        for (k, v) in captures {
                            let got = drafts[0].captures.as_ref().and_then(|c| c.get(k));
                            assert_eq!(got, Some(v), "{who}: capture {k}");
                        }
                    }
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "no pack examples were checked");
}
