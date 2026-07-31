//! Issue #6640: The Serpent Society's "Ward—Get five poison counters" was lowered
//! to a zero-mana ward (`WardCost::Mana` generic 0) because `WardCost` had no
//! poison-counter form, so an opponent targeting it paid nothing and the spell
//! resolved for free.
//!
//! Oracle text (verified from card data, per the issue):
//!   Deathtouch
//!   Ward—Get five poison counters. (A player with ten or more poison counters
//!   loses the game.)
//!   Whenever another creature you control with deathtouch dies, each opponent
//!   sacrifices a nontoken creature of their choice.
//!
//! These runtime regressions drive the real cast pipeline: an opponent targets
//! the warded creature, Ward triggers, and the opponent either takes the five
//! poison counters (spell survives) or declines (spell is countered).
//!
//! https://github.com/phase-rs/phase/issues/6640

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::phase::Phase;
use engine::types::player::PlayerCounterKind;
use engine::types::zones::Zone;

const SERPENT_SOCIETY: &str = "Deathtouch\nWard—Get five poison counters. (A player with ten or more poison counters loses the game.)\nWhenever another creature you control with deathtouch dies, each opponent sacrifices a nontoken creature of their choice.";

/// CR 702.21a + CR 122.1: targeting The Serpent Society triggers Ward, prompting
/// the targeting opponent to pay by getting five poison counters. Paying leaves
/// the targeted spell on the stack and adds exactly five poison counters — not
/// zero, which was the pre-fix behavior of the mislowered `Mana(0)` cost.
#[test]
fn serpent_society_ward_charges_five_poison_counters_when_paid() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let serpent = scenario
        .add_creature_from_oracle(P0, "The Serpent Society", 3, 3, SERPENT_SOCIETY)
        .id();
    let murder = scenario
        .add_spell_to_hand_from_oracle(P1, "Murder", true, "Destroy target creature.")
        .id();
    let mut runner = scenario.build();
    {
        let state = runner.state_mut();
        state.active_player = P1;
        state.priority_player = P1;
        state.waiting_for = WaitingFor::Priority { player: P1 };
    }

    runner.cast(murder).target_objects(&[serpent]).commit();
    runner.advance_until_stack_empty();

    // CR 702.21a: Ward must prompt the targeting opponent (P1) to pay.
    let WaitingFor::UnlessPayment { player, .. } = &runner.state().waiting_for else {
        panic!(
            "The Serpent Society's Ward must prompt the opponent, got {:?}",
            runner.state().waiting_for
        );
    };
    assert_eq!(*player, P1, "the targeting player pays Ward (CR 702.21a)");
    assert_eq!(
        runner.state().players[P1.0 as usize].poison_counters,
        0,
        "no poison counters before payment"
    );

    runner
        .act(GameAction::PayUnlessCost { pay: true })
        .expect("the opponent chooses to pay the poison-counter Ward cost");

    // CR 122.1 + CR 104.3d: paying adds exactly five poison counters (routed to
    // the dedicated poison field), not zero.
    assert_eq!(
        runner.state().players[P1.0 as usize].poison_counters,
        5,
        "paying Ward must give the opponent five poison counters"
    );
    assert_eq!(
        runner.state().players[P1.0 as usize].player_counter(&PlayerCounterKind::Poison),
        5,
        "poison accessor mirrors the dedicated field"
    );
    // CR 702.21a: paying Ward leaves the targeted spell on the stack to resolve.
    assert!(
        runner.state().stack.iter().any(|entry| entry.id == murder),
        "paying Ward keeps the targeting spell on the stack"
    );
}

/// CR 702.21a: declining the poison-counter Ward cost counters the targeting
/// spell (it never resolves), and the opponent gains no poison counters.
#[test]
fn serpent_society_ward_counters_the_spell_when_declined() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let serpent = scenario
        .add_creature_from_oracle(P0, "The Serpent Society", 3, 3, SERPENT_SOCIETY)
        .id();
    let murder = scenario
        .add_spell_to_hand_from_oracle(P1, "Murder", true, "Destroy target creature.")
        .id();
    let mut runner = scenario.build();
    {
        let state = runner.state_mut();
        state.active_player = P1;
        state.priority_player = P1;
        state.waiting_for = WaitingFor::Priority { player: P1 };
    }

    runner.cast(murder).target_objects(&[serpent]).commit();
    runner.advance_until_stack_empty();

    assert!(
        matches!(runner.state().waiting_for, WaitingFor::UnlessPayment { .. }),
        "Ward must prompt before the spell resolves, got {:?}",
        runner.state().waiting_for
    );

    runner
        .act(GameAction::PayUnlessCost { pay: false })
        .expect("the opponent declines the poison-counter Ward cost");

    // CR 702.21a + CR 701.6a: declining counters the spell to its owner's graveyard.
    assert_eq!(
        runner.state().objects[&murder].zone,
        Zone::Graveyard,
        "declining Ward counters the targeting spell to the graveyard"
    );
    // CR 122.1: no poison counters are gained when the cost is declined.
    assert_eq!(
        runner.state().players[P1.0 as usize].poison_counters,
        0,
        "declining Ward gives no poison counters"
    );
    // The warded creature survives — the destroy spell never resolved.
    assert_eq!(
        runner.state().objects[&serpent].zone,
        Zone::Battlefield,
        "the countered spell never destroys the warded creature"
    );
}
