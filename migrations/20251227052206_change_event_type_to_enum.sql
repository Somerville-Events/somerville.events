-- Create the enum type with all final values
CREATE TYPE app.event_type AS ENUM (
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
ALTER COLUMN event_type TYPE app.event_type
USING (
    CASE
        WHEN event_type IS NULL THEN NULL
        WHEN event_type = 'YardSale' THEN 'YardSale'::app.event_type
        WHEN event_type = 'Art' THEN 'Art'::app.event_type
        WHEN event_type = 'Music' THEN 'Music'::app.event_type
        WHEN event_type = 'Dance' THEN 'Dance'::app.event_type
        WHEN event_type = 'Performance' THEN 'Performance'::app.event_type
        WHEN event_type = 'Food' THEN 'Food'::app.event_type
        WHEN event_type = 'PersonalService' THEN 'PersonalService'::app.event_type
        WHEN event_type = 'Meeting' THEN 'Meeting'::app.event_type
        WHEN event_type = 'Government' THEN 'Government'::app.event_type
        WHEN event_type = 'Volunteer' THEN 'Volunteer'::app.event_type
        WHEN event_type = 'Fundraiser' THEN 'Fundraiser'::app.event_type
        WHEN event_type = 'Film' THEN 'Film'::app.event_type
        WHEN event_type = 'Theater' THEN 'Theater'::app.event_type
        WHEN event_type = 'Comedy' THEN 'Comedy'::app.event_type
        WHEN event_type = 'Literature' THEN 'Literature'::app.event_type
        WHEN event_type = 'Exhibition' THEN 'Exhibition'::app.event_type
        WHEN event_type = 'Workshop' THEN 'Workshop'::app.event_type
        WHEN event_type = 'Fitness' THEN 'Fitness'::app.event_type
        WHEN event_type = 'Market' THEN 'Market'::app.event_type
        WHEN event_type = 'Sports' THEN 'Sports'::app.event_type
        WHEN event_type = 'Family' THEN 'Family'::app.event_type
        WHEN event_type = 'Social' THEN 'Social'::app.event_type
        WHEN event_type = 'Holiday' THEN 'Holiday'::app.event_type
        WHEN event_type = 'Religious' THEN 'Religious'::app.event_type
        ELSE 'Other'::app.event_type
    END
);
