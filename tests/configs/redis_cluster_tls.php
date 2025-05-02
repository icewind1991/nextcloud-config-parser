<?php

$CONFIG = [
	'overwrite.cli.url' => 'https://cloud.example.com',
	'dbtype' => 'mysql',
	'dbname' => 'nextcloud',
	'dbhost' => '127.0.0.1',
	'dbport' => '',
	'dbtableprefix' => 'oc_',
	'dbuser' => 'nextcloud',
	'dbpassword' => 'secret',
     'redis.cluster' =>
      array (
        'seeds' =>
        array (
          0 => 'db1:6380',
          1 => 'db1:6381',
        ),
        'password' => 'xxx',
        'timeout' => 0.0,
        'read_timeout' => 0.0,
        'failover_mode' => \RedisCluster::FAILOVER_ERROR,
        'ssl_context' => [
          'local_cert' => '/certs/redis.crt',
          'local_pk' => '/certs/redis.key',
          'cafile' => '/certs/ca.crt'
        ]
      ),
];
