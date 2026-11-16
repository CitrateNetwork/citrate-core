// =====================================================================
// citrate-core — Groups (route groups/chat)
//
// 0.5.4 FL design, board 02: Groups › Chat is now surfaces/groups/GroupChat.tsx. The People and
// Cluster views that used to be tabs inside this surface are the place's own tabs: Groups › Members
// (surfaces/groups/GroupMembers.tsx) and Groups › Cluster (surfaces/Cluster.tsx). `Groups` stays the
// export the router and older imports use.
// =====================================================================
import type { SurfaceProps } from "./shared";
import { GroupChat } from "./groups/GroupChat";

export function Groups({ store, s }: SurfaceProps) {
  return <GroupChat store={store} s={s} view="groups" />;
}
