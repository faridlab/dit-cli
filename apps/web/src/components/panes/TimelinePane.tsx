// The Timeline sidebar section: what the feed is, and where its numbers come
// from. There is nothing to configure that is not already in the URL — the
// point in history rides in `?seq=`, so it is shareable and reloadable — so
// this section explains rather than controls.

import { SectionHeading } from "../chrome";
import { useActivitySummary } from "../../lib/queries";

export function TimelinePane({ seq }: { seq: number | null }) {
  const summary = useActivitySummary({ seq });
  const travelling = summary.data ? summary.data.seq < summary.data.max_seq : false;

  return (
    <div className="flex flex-col gap-4 p-3">
      <section>
        <SectionHeading size="sm" className="px-1 pb-2">
          Standing at
        </SectionHeading>
        <p className="px-1 font-mono text-[12px] text-ink-2">
          {travelling && summary.data ? `seq ${summary.data.seq}` : "now"}
        </p>
        <p className="px-1 pt-1 text-[11.5px] leading-relaxed text-muted">
          {travelling
            ? "Click a day in the chart to move, or use “Back to now”."
            : "Click a day in the chart to look at the workspace as it stood then."}
        </p>
      </section>

      {summary.data ? (
        <section>
          <SectionHeading size="sm" className="px-1 pb-2">
            Recorded history
          </SectionHeading>
          <dl className="flex flex-col gap-1 px-1 font-mono text-[11.5px] text-muted">
            <div className="flex justify-between">
              <dt>events</dt>
              <dd className="tabular-nums text-ink-2">{summary.data.max_seq}</dd>
            </div>
            <div className="flex justify-between">
              <dt>open now</dt>
              <dd className="tabular-nums text-ink-2">
                {summary.data.now.todo + summary.data.now.doing}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt>done now</dt>
              <dd className="tabular-nums text-ink-2">{summary.data.now.done}</dd>
            </div>
          </dl>
        </section>
      ) : null}

      <section>
        <SectionHeading size="sm" className="px-1 pb-2">
          Where this comes from
        </SectionHeading>
        <p className="px-1 text-[11.5px] leading-relaxed text-muted">
          Every line is a field change observed in a commit, so an edit made in a text editor
          appears exactly like one made here. The order is <span className="font-mono">seq</span>,
          the position in the commit graph — never the timestamp, which a merge commit makes
          contradict itself.
        </p>
      </section>
    </div>
  );
}
