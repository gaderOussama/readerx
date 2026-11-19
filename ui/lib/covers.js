// A book needs a cover probe when it is on disk, has no cover yet, was never
// checked (a coverless EPUB is checked once), and has not already failed this session.
export const needsProbe = (book, failed) =>
  book.present && !book.coverPath && !book.coverChecked && !failed.has(book.id);
