-- 1. Ensure all events with 'Family' also have 'ChildFriendly'
-- If they already have 'ChildFriendly', this does nothing (due to ON CONFLICT).
-- If they don't, it adds 'ChildFriendly'.
INSERT INTO app.event_event_types (event_id, event_type_name)
SELECT event_id, 'ChildFriendly'
FROM app.event_event_types
WHERE event_type_name = 'Family'
ON CONFLICT DO NOTHING;

-- 2. Remove all 'Family' associations
DELETE FROM app.event_event_types
WHERE event_type_name = 'Family';

-- 3. Remove 'Family' from the available event types
DELETE FROM app.event_types
WHERE name = 'Family';
