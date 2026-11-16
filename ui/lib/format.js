const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

// Calendar days in local time. Date.UTC on the local parts keeps DST changes
// from turning a 23 h or 25 h day into an off-by-one.
const dayNumber = (d) => Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / 86400000;

export function relativeDay(isoString, now = new Date()) {
  const date = new Date(isoString);
  const days = dayNumber(now) - dayNumber(date);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days <= 6) return `${days} days ago`;
  return `${date.getDate()} ${MONTHS[date.getMonth()]} ${date.getFullYear()}`;
}

export function progressText(book) {
  if (!book.position || !book.pageCount) return null;
  return `p. ${book.position.page} / ${book.pageCount} · ${Math.round(book.progress * 100)}%`;
}
