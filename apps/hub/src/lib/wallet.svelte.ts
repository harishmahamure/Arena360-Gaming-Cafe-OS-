export const walletEvents: { version: number; playerId: string } = $state({
  version: 0,
  playerId: "",
});

export function applyWalletChanged(playerId: string) {
  walletEvents.playerId = playerId;
  walletEvents.version++;
}

/** An Undo reached the Hub: any open profile re-reads what it owns (the player is unknown). */
export function applyBillReverted() {
  walletEvents.playerId = "";
  walletEvents.version++;
}
