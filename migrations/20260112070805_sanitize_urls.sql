-- Delete URLs that don't start with http:// or https://
UPDATE app.events 
SET url = NULL 
WHERE url IS NOT NULL 
  AND url !~ '^https?://';
