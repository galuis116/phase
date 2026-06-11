use crate::game::printed_cards::apply_back_face_to_object;
use crate::types::ability::{Effect, EffectError, EffectKind, ResolvedAbility};
use crate::types::events::GameEvent;
use crate::types::game_state::GameState;

/// CR 708.3: Turn the card(s) referenced by `target` face up via a resolving
/// effect — distinct from the morph/disguise *special action* in
/// `game/morph.rs::turn_face_up`. Used by the Imprint "flip" cards — Clone
/// Shell, Summoner's Egg, Compleated Clone Shell, The Creation of Avacyn —
/// which exile a card face down and later "turn the exiled card face up".
///
/// A card exiled face down keeps its real identity in exile (the face-down
/// profile is applied only on battlefield entry — see
/// `zone_pipeline::apply_face_down_entry_profile`), so for those cards clearing
/// the face-down flag is a no-op and this simply emits `TurnedFaceUp` (so any
/// "turned face up" trigger fires) — the conditional follow-up ("if it's a
/// creature card, put it onto the battlefield …") then reads the card's real
/// type and moves it. If a genuinely face-down carrier with a stored
/// `back_face` is targeted, its real characteristics are restored (CR 708.2a).
pub fn resolve(
    state: &mut GameState,
    ability: &ResolvedAbility,
    events: &mut Vec<GameEvent>,
) -> Result<(), EffectError> {
    let target = match &ability.effect {
        Effect::TurnFaceUp { target } => target.clone(),
        _ => return Ok(()),
    };

    let resolved = crate::game::targeting::resolved_targets(ability, &target, state);
    let object_ids = crate::game::effects::effect_object_targets(&target, &resolved);

    let mut restored_any = false;
    for id in object_ids {
        if let Some(obj) = state.objects.get_mut(&id) {
            if obj.face_down {
                obj.face_down = false;
                if let Some(back) = obj.back_face.take() {
                    apply_back_face_to_object(obj, back);
                }
                restored_any = true;
            }
        }
        events.push(GameEvent::TurnedFaceUp { object_id: id });
    }

    // CR 613: a turned-up card's restored characteristics require a layer
    // re-derive (mirrors the morph special-action path).
    if restored_any {
        crate::game::layers::mark_layers_full(state);
    }

    events.push(GameEvent::EffectResolved {
        kind: EffectKind::TurnFaceUp,
        source_id: ability.source_id,
    });
    Ok(())
}
