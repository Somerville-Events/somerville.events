-- Update app.source_names to match Rust Enum variants (PascalCase, no spaces)

-- Temporarily disable triggers or constraints if necessary (here we just update foreign keys implicitly if we had cascading updates, but we likely need to handle it manually or update the reference table)

-- Since app.events.source_name references app.source_names(name), we must update the parent table first or handle the constraint.
-- However, updating the primary key of a referenced table is tricky.
-- Best approach:
-- 1. Insert new names
-- 2. Update events to point to new names
-- 3. Delete old names

INSERT INTO app.source_names (name) VALUES
('AeronautBrewing'),
('AmericanRepertoryTheater'),
('ArtsAtTheArmory'),
('BostonSwingCentral'),
('BostonShowsOrg'),
('BrattleTheatre'),
('CentralSquareTheater'),
('CityOfCambridge'),
('HarvardArtMuseums'),
('HarvardBookStore'),
('LamplighterBrewing'),
('PorterSquareBooks'),
('PorticoBrewing'),
('SandersTheatre'),
('SomervilleTheatre'),
('TheComedyStudio'),
('TheDanceComplex'),
('TheLilyPad'),
('TheMiddleEast')
ON CONFLICT DO NOTHING;

-- Update events to use new names
UPDATE app.events SET source_name = 'AeronautBrewing' WHERE source_name = 'Aeronaut Brewing';
UPDATE app.events SET source_name = 'AmericanRepertoryTheater' WHERE source_name = 'American Repertory Theater';
UPDATE app.events SET source_name = 'ArtsAtTheArmory' WHERE source_name = 'Arts at the Armory';
UPDATE app.events SET source_name = 'BostonSwingCentral' WHERE source_name = 'Boston Swing Central';
UPDATE app.events SET source_name = 'BostonShowsOrg' WHERE source_name = 'BostonShows.org';
UPDATE app.events SET source_name = 'BrattleTheatre' WHERE source_name = 'Brattle Theatre';
UPDATE app.events SET source_name = 'CentralSquareTheater' WHERE source_name = 'Central Square Theater';
UPDATE app.events SET source_name = 'CityOfCambridge' WHERE source_name = 'City of Cambridge';
UPDATE app.events SET source_name = 'HarvardArtMuseums' WHERE source_name = 'Harvard Art Museums';
UPDATE app.events SET source_name = 'HarvardBookStore' WHERE source_name = 'Harvard Book Store';
UPDATE app.events SET source_name = 'LamplighterBrewing' WHERE source_name = 'Lamplighter Brewing';
UPDATE app.events SET source_name = 'PorterSquareBooks' WHERE source_name = 'Porter Square Books';
UPDATE app.events SET source_name = 'PorticoBrewing' WHERE source_name = 'Portico Brewing';
UPDATE app.events SET source_name = 'SandersTheatre' WHERE source_name = 'Sanders Theatre';
UPDATE app.events SET source_name = 'SomervilleTheatre' WHERE source_name = 'Somerville Theatre';
UPDATE app.events SET source_name = 'TheComedyStudio' WHERE source_name = 'The Comedy Studio';
UPDATE app.events SET source_name = 'TheDanceComplex' WHERE source_name = 'The Dance Complex';
UPDATE app.events SET source_name = 'TheLilyPad' WHERE source_name = 'The Lily Pad';
UPDATE app.events SET source_name = 'TheMiddleEast' WHERE source_name = 'The Middle East';

-- Delete old names that are no longer referenced
DELETE FROM app.source_names WHERE name IN (
    'Aeronaut Brewing',
    'American Repertory Theater',
    'Arts at the Armory',
    'Boston Swing Central',
    'BostonShows.org',
    'Brattle Theatre',
    'Central Square Theater',
    'City of Cambridge',
    'Harvard Art Museums',
    'Harvard Book Store',
    'Lamplighter Brewing',
    'Porter Square Books',
    'Portico Brewing',
    'Sanders Theatre',
    'Somerville Theatre',
    'The Comedy Studio',
    'The Dance Complex',
    'The Lily Pad',
    'The Middle East'
);
