// The Search sidebar section is the Issues one: the same filters, the same
// saved views. Search results are issues, and a saved view is a search, so
// the two screens share their side rail rather than teaching two vocabularies.
// The query box itself lives in the view, where the results are.

import { IssuesPane } from "./IssuesPane";

export function SearchPane({
  q: _q,
  onSearch: _onSearch,
}: {
  /** Legacy: the query box moved into the view. Kept so the shell's call
   *  site keeps compiling. */
  q: string;
  onSearch: (q: string) => void;
}) {
  return <IssuesPane q={null} onFilter={() => undefined} />;
}
