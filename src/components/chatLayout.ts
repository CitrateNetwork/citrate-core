// The chat's bottom zone. HGW (v0.5.1) removed the rc.5 viewport measuring (the pane now fills
// its column by CSS; see sectionLayout.tsx) and moved the follow logic to useStickToBottom.ts.

/** Within ~40 px of the bottom counts as "at the latest message". */
export function shouldFollow(el: { scrollHeight: number; scrollTop: number; clientHeight: number }): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight < 40;
}
