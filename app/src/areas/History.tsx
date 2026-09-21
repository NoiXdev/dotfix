import Empty from "../components/Empty";
import type { Commit } from "../types";

/**
 * The commit log for this machine's configuration repository, newest first
 * (the backend already orders `history` that way). Purely presentational —
 * App loads the commits once the tab is opened and hands them down.
 */
export default function History({ commits }: { commits: Commit[] }) {
  if (commits.length === 0) return <Empty>No commits yet</Empty>;

  return (
    <ul>
      {commits.map((commit) => (
        <li
          key={commit.hash}
          className="flex items-baseline justify-between gap-3 border-b border-neutral-100 py-2"
        >
          <span className="min-w-0 truncate">{commit.subject}</span>
          <span className="shrink-0 font-mono text-xs text-neutral-500">
            {commit.date} {commit.hash}
          </span>
        </li>
      ))}
    </ul>
  );
}
