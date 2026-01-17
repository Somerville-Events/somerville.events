INSERT INTO app.event_types (name) VALUES
('Trivia'),
('BoardGames'),
('Bikes')
ON CONFLICT DO NOTHING;
