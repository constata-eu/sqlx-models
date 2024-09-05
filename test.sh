#!/usr/bin/sh
docker run --name sqlx_models_derive_test_db --rm -e POSTGRES_USER=sqlx_models_derive -e POSTGRES_PASSWORD=password -e POSTGRES_DB=sqlx_models_derive -p 5432:5432 -d postgres
cd sqlx-models-orm && sqlx database reset && cd - && cargo test
docker stop sqlx_models_derive_test_db
