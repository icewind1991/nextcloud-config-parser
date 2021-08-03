<?php

$CONFIG = [
	'overwrite.cli.url' => 'https://cloud.example.com',
	'dbtype' => 'mysql',
	'dbname' => 'nextcloud',
	'dbhost' => '1.2.3.4',
	'dbport' => '',
	'dbtableprefix' => 'oc_',
	'dbuser' => 'nextcloud',
	'dbpassword' => 'secret',
	'redis' => [
		'host' => 'localhost'
	],
	'dbdriveroptions' => [
      \PDO::MYSQL_ATTR_SSL_VERIFY_SERVER_CERT => false,
    ],
];
