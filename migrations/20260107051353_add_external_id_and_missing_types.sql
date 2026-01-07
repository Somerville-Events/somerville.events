-- Add url column to source_names
ALTER TABLE app.source_names ADD COLUMN url TEXT;

-- Add new sources
INSERT INTO app.source_names (name) VALUES
('GrolierPoetryBookShop'),
('FirstParishInCambridge'),
('UserSubmitted')
ON CONFLICT DO NOTHING;

-- REMOVED: Lecture and Community additions
-- INSERT INTO app.event_types (name) VALUES
-- ('Lecture'),
-- ('Community')
-- ON CONFLICT DO NOTHING;

-- Add external_id to events
ALTER TABLE app.events ADD COLUMN external_id TEXT;
CREATE UNIQUE INDEX idx_events_source_external_id ON app.events (source, external_id);

-- Populate source URLs
UPDATE app.source_names SET url = 'https://aeronautbrewing.com' WHERE name = 'AeronautBrewing';
UPDATE app.source_names SET url = 'https://americanrepertorytheater.org' WHERE name = 'AmericanRepertoryTheater';
UPDATE app.source_names SET url = 'https://artsatthearmory.org' WHERE name = 'ArtsAtTheArmory';
UPDATE app.source_names SET url = 'https://www.bostonswingcentral.org' WHERE name = 'BostonSwingCentral';
UPDATE app.source_names SET url = 'https://bostonshows.org' WHERE name = 'BostonShowsOrg';
UPDATE app.source_names SET url = 'https://brattlefilm.org' WHERE name = 'BrattleTheatre';
UPDATE app.source_names SET url = 'https://www.centralsquaretheater.org' WHERE name = 'CentralSquareTheater';
UPDATE app.source_names SET url = 'https://www.cambridgema.gov' WHERE name = 'CityOfCambridge';
UPDATE app.source_names SET url = 'https://harvardartmuseums.org' WHERE name = 'HarvardArtMuseums';
UPDATE app.source_names SET url = 'https://www.harvard.com' WHERE name = 'HarvardBookStore';
UPDATE app.source_names SET url = 'https://lamplighterbrewing.com' WHERE name = 'LamplighterBrewing';
UPDATE app.source_names SET url = 'https://www.portersquarebooks.com' WHERE name = 'PorterSquareBooks';
UPDATE app.source_names SET url = 'https://porticobrewing.com' WHERE name = 'PorticoBrewing';
UPDATE app.source_names SET url = 'https://boxoffice.harvard.edu' WHERE name = 'SandersTheatre';
UPDATE app.source_names SET url = 'https://www.somervilletheatre.com' WHERE name = 'SomervilleTheatre';
UPDATE app.source_names SET url = 'https://www.thecomedystudio.com' WHERE name = 'TheComedyStudio';
UPDATE app.source_names SET url = 'https://www.dancecomplex.org' WHERE name = 'TheDanceComplex';
UPDATE app.source_names SET url = 'https://www.lilypadinman.com' WHERE name = 'TheLilyPad';
UPDATE app.source_names SET url = 'https://mideastoffers.com' WHERE name = 'TheMiddleEast';
UPDATE app.source_names SET url = 'https://www.grolierpoetrybookshop.com' WHERE name = 'GrolierPoetryBookShop';
UPDATE app.source_names SET url = 'https://firstparishcambridge.org' WHERE name = 'FirstParishInCambridge';
