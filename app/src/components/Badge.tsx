/**
 * A count next to an area or set name. Any non-zero count means something
 * there wants a decision, so it always carries the one "decision" accent —
 * never a plain neutral pill — and disappears entirely at zero rather than
 * showing a hollow "0".
 */
export default function Badge({ count }: { count: number }) {
  if (count === 0) return null;
  return (
    <span className="ml-1 rounded-full bg-decision-soft px-1.5 font-mono text-xs tabular-nums text-decision">
      {count}
    </span>
  );
}
