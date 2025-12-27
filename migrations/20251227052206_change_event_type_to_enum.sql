-- Create the enum type with all final values
CREATE TYPE event_type AS ENUM (
    'YardSale',
    'Art',
    'Music',
    'Dance',
    'Performance',
    'Food',
    'PersonalService',
    'Meeting',
    'Government',
    'Volunteer',
    'Fundraiser',
    'Film',
    'Theater',
    'Comedy',
    'Literature',
    'Exhibition',
    'Workshop',
    'Fitness',
    'Market',
    'Sports',
    'Family',
    'Social',
    'Holiday',
    'Religious',
    'Other'
);

-- Alter the table to use the new enum type
ALTER TABLE app.events
ALTER COLUMN event_type TYPE event_type
USING (
    CASE
        WHEN event_type IS NULL THEN NULL
        WHEN event_type = 'YardSale' THEN 'YardSale'::event_type
        WHEN event_type = 'Art' THEN 'Art'::event_type
        WHEN event_type = 'Music' THEN 'Music'::event_type
        WHEN event_type = 'Dance' THEN 'Dance'::event_type
        WHEN event_type = 'Performance' THEN 'Performance'::event_type
        WHEN event_type = 'Food' THEN 'Food'::event_type
        WHEN event_type = 'PersonalService' THEN 'PersonalService'::event_type
        WHEN event_type = 'Meeting' THEN 'Meeting'::event_type
        WHEN event_type = 'Government' THEN 'Government'::event_type
        WHEN event_type = 'Volunteer' THEN 'Volunteer'::event_type
        WHEN event_type = 'Fundraiser' THEN 'Fundraiser'::event_type
        WHEN event_type = 'Film' THEN 'Film'::event_type
        WHEN event_type = 'Theater' THEN 'Theater'::event_type
        WHEN event_type = 'Comedy' THEN 'Comedy'::event_type
        WHEN event_type = 'Literature' THEN 'Literature'::event_type
        WHEN event_type = 'Exhibition' THEN 'Exhibition'::event_type
        WHEN event_type = 'Workshop' THEN 'Workshop'::event_type
        WHEN event_type = 'Fitness' THEN 'Fitness'::event_type
        WHEN event_type = 'Market' THEN 'Market'::event_type
        WHEN event_type = 'Sports' THEN 'Sports'::event_type
        WHEN event_type = 'Family' THEN 'Family'::event_type
        WHEN event_type = 'Social' THEN 'Social'::event_type
        WHEN event_type = 'Holiday' THEN 'Holiday'::event_type
        WHEN event_type = 'Religious' THEN 'Religious'::event_type
        ELSE 'Other'::event_type
    END
);
