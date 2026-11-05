-- Who set the meeting title: 'user' (typed by the user) or 'auto' (made from the
-- finished report). NULL is a title from before this column existed; it is only
-- replaced while it still is the default name.
ALTER TABLE meetings ADD COLUMN title_source TEXT CHECK (title_source IN ('user', 'auto'));
