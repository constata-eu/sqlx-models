#!/usr/bin/sh
cd sqlx-models-orm && sqlx database reset && cd - && cargo test
