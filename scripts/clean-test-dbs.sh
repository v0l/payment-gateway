#!/usr/bin/env bash
set -euo pipefail

container="pgw-test-db"
older_than=0
while [ $# -gt 0 ]; do
  case "$1" in
    --older-than) older_than="$2"; shift 2 ;;
    *) container="$1"; shift ;;
  esac
done
password="${MARIADB_ROOT_PASSWORD:-test}"

{
  echo "SET @older_than = ${older_than};"
  cat <<'SQL'
DELIMITER //
BEGIN NOT ATOMIC
  DECLARE done INT DEFAULT 0;
  DECLARE db VARCHAR(64);
  DECLARE cur CURSOR FOR
    SELECT s.schema_name
    FROM information_schema.SCHEMATA s
    WHERE s.schema_name LIKE 't\_%'
      AND NOT EXISTS (SELECT 1 FROM information_schema.PROCESSLIST p WHERE p.DB = s.schema_name)
      AND (@older_than = 0 OR COALESCE((
            SELECT MAX(t.CREATE_TIME) FROM information_schema.TABLES t WHERE t.TABLE_SCHEMA = s.schema_name
          ), '1970-01-01') < NOW() - INTERVAL @older_than MINUTE);
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
} | docker exec -i "$container" mariadb -uroot -p"$password"
