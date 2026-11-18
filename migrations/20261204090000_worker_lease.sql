CREATE TABLE worker_lease (
    name VARCHAR(64) NOT NULL PRIMARY KEY,
    holder VARCHAR(128) NOT NULL,
    acquired TIMESTAMP NOT NULL,
    expires TIMESTAMP NOT NULL
);
