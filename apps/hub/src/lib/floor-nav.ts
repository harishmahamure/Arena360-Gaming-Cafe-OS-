export function nextTile(tiles: HTMLElement[], from: number, key: string): number {
  if (key === "ArrowRight") return Math.min(from + 1, tiles.length - 1);
  if (key === "ArrowLeft") return Math.max(from - 1, 0);
  const sign = key === "ArrowDown" ? 1 : -1;
  const r = tiles[from].getBoundingClientRect();
  let best = from;
  let score = Infinity;
  tiles.forEach((el, i) => {
    const b = el.getBoundingClientRect();
    const dy = (b.top - r.top) * sign;
    const s = dy * 1e4 + Math.abs(b.left - r.left);
    if (dy >= 1 && s < score) [best, score] = [i, s];
  });
  return best;
}
