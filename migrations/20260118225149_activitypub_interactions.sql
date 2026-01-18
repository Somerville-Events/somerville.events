CREATE TABLE app.activitypub_followers (
    id BIGSERIAL PRIMARY KEY,
    actor_id TEXT NOT NULL UNIQUE,
    actor_url TEXT NOT NULL,
    inbox_url TEXT NOT NULL,
    shared_inbox_url TEXT NULL,
    public_key_pem TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX activitypub_followers_actor_id_idx
    ON app.activitypub_followers (actor_id);

CREATE TABLE app.activitypub_inbox_activities (
    id BIGSERIAL PRIMARY KEY,
    activity_id TEXT NOT NULL UNIQUE,
    activity_type TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    object_id TEXT NULL,
    object_type TEXT NULL,
    object_url TEXT NULL,
    object_content TEXT NULL,
    object_published TIMESTAMPTZ NULL,
    in_reply_to TEXT NULL,
    event_id BIGINT NULL REFERENCES app.events (id) ON DELETE CASCADE,
    payload JSONB NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX activitypub_inbox_activities_event_id_idx
    ON app.activitypub_inbox_activities (event_id);

CREATE INDEX activitypub_inbox_activities_activity_type_idx
    ON app.activitypub_inbox_activities (activity_type);

CREATE INDEX activitypub_inbox_activities_actor_id_idx
    ON app.activitypub_inbox_activities (actor_id);

CREATE TABLE app.activitypub_event_rsvps (
    id BIGSERIAL PRIMARY KEY,
    activity_id TEXT NOT NULL UNIQUE,
    event_id BIGINT NOT NULL REFERENCES app.events (id) ON DELETE CASCADE,
    actor_id TEXT NOT NULL,
    rsvp_type TEXT NOT NULL,
    object_id TEXT NULL,
    payload JSONB NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (event_id, actor_id)
);

CREATE INDEX activitypub_event_rsvps_event_id_idx
    ON app.activitypub_event_rsvps (event_id);

CREATE INDEX activitypub_event_rsvps_actor_id_idx
    ON app.activitypub_event_rsvps (actor_id);
