<?php

$CONFIG = [
	'overwrite.cli.url' => 'https://cloud.example.com',
	'dbtype' => 'pgsql',
  'dbname' => 'nextcloud',
  'dbhost' => '/run/postgresql',
  'dbport' => '',
  'dbtableprefix' => 'oc_',
  'dbuser' => 'nextcloud',
  'dbpassword' => 'redacted',
	'redis' => [
		'host' => 'localhost'
	]
];
