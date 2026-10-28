/** Drafts in the capability page stay mounted across top-level navigation. */
export function blocksConnectionNavigation(state:{blocked:boolean;capabilityPage:boolean;busy:boolean;pending:boolean;confirmDiscard:boolean}):boolean {
  return state.busy || state.pending || state.confirmDiscard || (state.blocked && !state.capabilityPage);
}
