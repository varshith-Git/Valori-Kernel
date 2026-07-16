/**
 * Single source of truth for event-type theming.
 * Three consumers previously kept independent copies:
 *   - app/page.tsx          (EVENT_DOT  — dot colours)
 *   - app/search/page.tsx   (EVENT_COLORS — text colours)
 *   - app/auditor/page.tsx  (TYPE_COLORS — badge colours)
 */

/** Tailwind dot colour for timeline activity feeds. */
export const EVENT_DOT: Record<string, string> = {
  InsertRecord:     "bg-emerald-500 dark:bg-emerald-400",
  SoftDeleteRecord: "bg-amber-500 dark:bg-amber-400",
  DeleteRecord:     "bg-red-500 dark:bg-red-400",
  CreateNode:       "bg-blue-500 dark:bg-blue-400",
  CreateEdge:       "bg-purple-500 dark:bg-purple-400",
  DeleteNode:       "bg-red-500 dark:bg-red-400",
  DeleteEdge:       "bg-red-500 dark:bg-red-400",
  CreateNamespace:  "bg-sky-500 dark:bg-sky-400",
  DropNamespace:    "bg-orange-500 dark:bg-orange-400",
};

/** Tailwind text colour for inline event-type labels. */
export const EVENT_COLORS: Record<string, string> = {
  InsertRecord:     "text-emerald-600 dark:text-emerald-400",
  SoftDeleteRecord: "text-amber-600   dark:text-amber-400",
  DeleteRecord:     "text-red-600     dark:text-red-400",
  CreateNode:       "text-blue-600    dark:text-blue-400",
  CreateEdge:       "text-purple-600  dark:text-purple-400",
  DeleteNode:       "text-red-600     dark:text-red-400",
  DeleteEdge:       "text-red-600     dark:text-red-400",
  CreateNamespace:  "text-sky-600     dark:text-sky-400",
  DropNamespace:    "text-orange-600  dark:text-orange-400",
};

/** Tailwind badge (bg + border + text) for auditor-style event tables.
 *  Keys use the normalised uppercase form emitted by the auditor API. */
export const EVENT_BADGE: Record<string, string> = {
  INSERT:      "bg-emerald-500/15 text-emerald-700 border-emerald-500/30 dark:text-emerald-400",
  DELETE:      "bg-red-500/15 text-red-700 border-red-500/30 dark:text-red-400",
  SOFT_DELETE: "bg-amber-500/15 text-amber-700 border-amber-500/30 dark:text-amber-400",
  NODE:        "bg-blue-500/15 text-blue-700 border-blue-500/30 dark:text-blue-400",
  EDGE:        "bg-purple-500/15 text-purple-700 border-purple-500/30 dark:text-purple-400",
  UNKNOWN:     "bg-card text-muted-foreground border-border",
};
