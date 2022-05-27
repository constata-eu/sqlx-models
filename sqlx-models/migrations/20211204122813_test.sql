CREATE TABLE humans (
  id SERIAL PRIMARY KEY NOT NULL,
  name VARCHAR NOT NULL,
  age INTEGER,
  is_allowed_unlimited_cats BOOLEAN NOT NULL DEFAULT FALSE,
  likes_dogs_too BOOLEAN NOT NULL
);

CREATE TYPE Personality AS ENUM (
  'Active',
  'Sleepy',
  'Playful',
  'Chaotic'
);

CREATE TABLE cats (
  id VARCHAR PRIMARY KEY NOT NULL,
  personality Personality NOT NULL,
  human_id INTEGER
);

CREATE TABLE toys (
  id SERIAL PRIMARY KEY NOT NULL,
  name VARCHAR NOT NULL,
  human_owner INTEGER
);

CREATE TABLE cats_toys (
  id SERIAL PRIMARY KEY NOT NULL,
  cat_id VARCHAR NOT NULL,
  toy_id INTEGER NOT NULL
);
