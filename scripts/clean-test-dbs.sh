#!/usr/bin/env bash
set -euo pipefail

container="${1:-pgw-test-db}"
password="${MARIADB_ROOT_PASSWORD:-test}"

docker exec -i "$container" mariadb -uroot -p"$password" <<'SQL'
DELIMITER //
BEGIN NOT ATOMIC
  DECLARE done INT DEFAULT 0;
  DECLARE db VARCHAR(64);
  DECLARE cur CURSOR FOR SELECT schema_name FROM information_schema.SCHEMATA WHERE schema_name LIKE 't\_%';
  DECLARE CONTINUE HANDLER FOR NOT FOUND SET done = 1;
  OPEN cur;
  drop_loop: LOOP
    FETCH cur INTO db;
    IF done THEN LEAVE drop_loop; END IF;
    SET @stmt = CONCAT('DROP DATABASE IF EXISTS `', db, '`');
    PREPARE s FROM @stmt;
    EXECUTE s;
    DEALLOCATE PREPARE s;
  END LOOP;
  CLOSE cur;
END//
DELIMITER ;
SQL
