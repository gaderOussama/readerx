// A book needs a cover probe when it is on disk, has no cover yet, and has not
// already failed this session.
export const needsProbe = (book, failed) => book.present && !book.coverPath && !failed.has(book.id);
