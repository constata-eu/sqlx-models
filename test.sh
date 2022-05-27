#!/usr/bin/sh
cd sqlx-models && sqlx database reset && cd - && cargo test
