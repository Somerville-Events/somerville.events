-- Update app.event_types to match Rust Enum variants (PascalCase, no spaces)

-- Insert new variants
INSERT INTO app.event_types (name) VALUES
('YardSale'),
('PersonalService'),
('ChildFriendly')
ON CONFLICT DO NOTHING;

-- Update join table
UPDATE app.event_event_types SET event_type_name = 'YardSale' WHERE event_type_name = 'Yard Sale';
UPDATE app.event_event_types SET event_type_name = 'PersonalService' WHERE event_type_name = 'Personal Service';
UPDATE app.event_event_types SET event_type_name = 'ChildFriendly' WHERE event_type_name = 'Child Friendly';

-- Delete old variants
DELETE FROM app.event_types WHERE name IN (
    'Yard Sale',
    'Personal Service',
    'Child Friendly'
);
