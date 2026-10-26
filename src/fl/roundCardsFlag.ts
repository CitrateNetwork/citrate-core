// FL round cards (0.5.2, first cut): the feature flag.
//
// The app has no runtime feature-flag system, so this is one build-time switch, default OFF:
// `VITE_FL_ROUND_CARDS=1` at build time (vite inlines it) turns the round cards on in Groups ›
// Rounds and in group chats. Any other value, or none, is OFF, and OFF renders exactly what the app
// rendered before the cards existed. See docs/fl/GROUP_ROUND_CARDS.md.
export function flRoundCardsEnabled(): boolean {
  return import.meta.env.VITE_FL_ROUND_CARDS === "1";
}
