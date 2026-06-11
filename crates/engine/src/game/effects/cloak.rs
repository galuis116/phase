use crate::game::quantity::resolve_quantity_with_targets;
use crate::types::ability::{Effect, EffectError, EffectKind, ResolvedAbility};
use crate::types::events::GameEvent;
use crate::types::game_state::GameState;

/// CR 702.170a: Cloak — put the top card of a player's library onto the
/// battlefield face down as a 2/2 creature **with ward {2}**. Like manifest
/// (CR 701.40a), a cloaked creature card can later be turned face up for its
/// mana cost; the sole behavioral difference is the ward {2} the cloaked
/// permanent enters with (granted via `FaceDownProfile::cloaked_2_2`).
///
/// `target` selects whose library is cloaked from (mirrors `Effect::Manifest`):
/// `Controller` for "you cloak the top card of your library",
/// `ParentTargetController` / `TriggeringPlayer` for relative-player bodies.
/// This first pass covers the top-of-library source; cloaking a card from hand
/// or a face-down pile is deferred (those need a player-selected source).
pub fn resolve(
    state: &mut GameState,
    ability: &ResolvedAbility,
    events: &mut Vec<GameEvent>,
) -> Result<(), EffectError> {
    let (target, count) = match &ability.effect {
        Effect::Cloak { target, count } => (
            target.clone(),
            resolve_quantity_with_targets(state, count, ability).max(0) as usize,
        ),
        _ => return Err(EffectError::MissingParam("count".to_string())),
    };

    let player = super::resolve_player_for_context_ref(state, ability, &target);

    // CR 701.40e (applied by analogy to cloak): cloak cards one at a time.
    for _ in 0..count {
        let has_cards = state
            .players
            .iter()
            .find(|p| p.id == player)
            .map(|p| !p.library.is_empty())
            .unwrap_or(false);

        if !has_cards {
            break;
        }

        crate::game::morph::cloak(state, player, events)
            .map_err(|e| EffectError::MissingParam(format!("{e}")))?;
    }

    events.push(GameEvent::EffectResolved {
        kind: EffectKind::from(&ability.effect),
        source_id: ability.source_id,
    });

    Ok(())
}
