<?php

$CONFIG = [
	'overwrite.cli.url' => 'https://cloud.example.com',
	'dbtype' => 'mysql',
	'dbname' => 'nextcloud',
	'dbhost' => 'localhost',
	'dbport' => '',
	'dbuser' => 'nextcloud',
	'dbpassword' => 'secret',
	'redis' => [
		'host' => 'localhost',
    'ssl_context' => [
      'local_cert' => '/certs/redis.crt',
      'local_pk' => '/certs/redis.key',
      'cafile' => '/certs/ca.crt'
    ]
	]
];
