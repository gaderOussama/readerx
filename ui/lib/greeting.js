// Greeting by local hour (0 to 23).
export function greeting(hour) {
  if (hour >= 5 && hour <= 11) return "Good morning";
  if (hour >= 12 && hour <= 17) return "Good afternoon";
  if (hour >= 18 && hour <= 22) return "Good evening";
  return "Good night";
}

// "2 in progress · 1 finished · 14 on the shelf"; zero parts are skipped.
export function shelfSummary(books) {
  const count = (section) => books.filter((b) => b.section === section).length;
  const parts = [
    [count("reading"), "in progress"],
    [count("finished"), "finished"],
    [books.length, "on the shelf"],
  ];
  return parts.filter(([n]) => n > 0).map(([n, label]) => `${n} ${label}`).join(" · ");
}
