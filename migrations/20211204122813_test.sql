CREATE TABLE persons  (
  id SERIAL PRIMARY KEY NOT NULL,
  name VARCHAR NOT NULL,
  alias VARCHAR,
  height_in_meters DECIMAL NOT NULL,
  has_drivers_license BOOLEAN NOT NULL,
  agreed_to_terms BOOLEAN,
  stringified_field INTEGER
);

