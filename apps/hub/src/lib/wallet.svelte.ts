export const walletEvents: { version: number; playerId: string } = $state({
  version: 0,
  playerId: "",
});

export function applyWalletChanged(playerId: string) {
  walletEvents.playerId = playerId;
  walletEvents.version++;
}
