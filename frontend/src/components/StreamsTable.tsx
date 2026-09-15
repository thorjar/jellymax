import type { MediaStream } from "../api/types";

interface StreamsTableProps {
  title: string;
  streams: MediaStream[];
  fallback: string;
}

export function StreamsTable({ title, streams, fallback }: StreamsTableProps) {
  return (
    <div className="rounded-xl border border-edge bg-surface-raised p-4">
      <h2 className="mb-3 text-sm font-semibold uppercase tracking-wider text-ink-muted">
        {title}
      </h2>
      {streams.length === 0 ? (
        <p className="text-sm text-ink-muted">{fallback}</p>
      ) : (
        <table className="w-full text-left text-sm">
          <thead>
            <tr className="border-b border-edge text-xs uppercase tracking-wider text-ink-muted">
              <th className="py-2 pr-4 font-medium">Index</th>
              <th className="py-2 pr-4 font-medium">Codec</th>
              <th className="py-2 pr-4 font-medium">Language</th>
              <th className="py-2 font-medium">Details</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-edge">
            {streams.map((stream) => (
              <tr key={stream.Index ?? `${stream.Type}-${stream.Codec}`} className="text-ink">
                <td className="py-2 pr-4 text-ink-muted">{stream.Index ?? "—"}</td>
                <td className="py-2 pr-4">{stream.Codec ?? "—"}</td>
                <td className="py-2 pr-4">{stream.Language ?? "—"}</td>
                <td className="py-2">
                  {stream.Type === "Video" &&
                    stream.Width &&
                    `${stream.Width}×${stream.Height ?? "?"}`}
                  {stream.Type === "Audio" &&
                    stream.Channels &&
                    `${stream.Channels} ch`}
                  {stream.Type === "Audio" &&
                    stream.SampleRate &&
                    ` · ${stream.SampleRate} Hz`}
                  {stream.IsExternal && " · external"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
