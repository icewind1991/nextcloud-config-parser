<?php

$CONFIG = [
	'overwrite.cli.url' => 'https://cloud.example.com',
	'dbtype' => 'mysql',
	'dbname' => 'nextcloud',
	'dbhost' => 'db.example.com',
	'dbport' => '',
	'dbtableprefix' => 'oc_',
	'dbuser' => 'nextcloud',
	'dbpassword' => 'secret',
	'redis' => [
		'host' => 'localhost'
	],
	'dbdriveroptions' => [
	  \PDO::MYSQL_ATTR_SSL_KEY => '/ssl-key.pem',
      \PDO::MYSQL_ATTR_SSL_CERT => '/ssl-cert.pem',
      \PDO::MYSQL_ATTR_SSL_CA => '/ca-cert.pem',
      \PDO::MYSQL_ATTR_SSL_VERIFY_SERVER_CERT => false,
    ],
];
