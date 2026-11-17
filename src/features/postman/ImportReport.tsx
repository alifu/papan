import { useState } from "react";
import type { ReportItem } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { reportError, showToast } from "../../ui/toastStore";
import { copyText } from "../response/clipboard";
import {
  filterReport,
  groupReport,
  REPORT_LEVELS,
  reportCounts,
  reportToText,
  type ReportFilter,
} from "./postmanModel";

export interface ImportReportProps {
  readonly report: readonly ReportItem[];
  /** Items the Rust side dropped to stay within the report limit. */
  readonly truncated?: number;
}

/** Report of an import or export: filter by level, grouped by file, copy as text. */
export function ImportReport({ report, truncated = 0 }: ImportReportProps) {
  const [filter, setFilter] = useState<ReportFilter>("all");
  const counts = reportCounts(report);
  const groups = groupReport(filterReport(report, filter));

  const copy = async () => {
    try {
      await copyText(reportToText(filterReport(report, filter)));
      showToast({ kind: "info", message: "Report copied." });
    } catch (err) {
      reportError(err);
    }
  };

  if (report.length === 0) return <p>Nothing to report.</p>;
  return (
    <section aria-label="Report" className="postman-report">
      <div className="postman-report-bar">
        <label>
          Show
          <select aria-label="Report level" value={filter} onChange={(e) => setFilter(e.target.value as ReportFilter)}>
            <option value="all">All ({report.length})</option>
            {REPORT_LEVELS.map((level) => (
              <option key={level} value={level}>
                {level} ({counts[level]})
              </option>
            ))}
          </select>
        </label>
        <Button label="Copy report" onClick={() => void copy()} />
      </div>
      {groups.map((group) => (
        <div key={group.file}>
          <h4>{group.file}</h4>
          <ul>
            {group.items.map((item, i) => (
              <li key={i} data-level={item.level}>
                <strong>{item.level}</strong> {item.path ? <code>{item.path}</code> : null} {item.message}
              </li>
            ))}
          </ul>
        </div>
      ))}
      {truncated > 0 ? <p>{truncated} more items were left out of the report.</p> : null}
    </section>
  );
}
