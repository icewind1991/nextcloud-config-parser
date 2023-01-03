<?php

function get_foo(): string { return 'foo'; }

$CONFIG = [
	'overwrite.cli.url' => 'https://cloud.example.com',
	'dbtype' => 'mysql',
	'dbname' => get_foo() . '_db',
	'dbhost' => '127.0.0.1',
	'dbport' => '',
	'dbtableprefix' => 'oc_',
	'dbuser' => 'nextcloud',
	'dbpassword' => 'secret',
	'redis' => [
		'host' => 'localhost'
	]
];