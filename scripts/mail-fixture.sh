#!/bin/bash
# Disposable container only: installs a fixture certificate and mail configuration.
set -euo pipefail
test -f /.dockerenv || { printf 'Run only in a disposable Docker container.\n' >&2; exit 1; }
useradd -m -u 1500 alice
mkdir -p /work/test-cert
openssl req -x509 -newkey rsa:2048 -nodes -keyout /work/test-cert/server.key -out /work/test-cert/server.crt -days 2 -subj /CN=localhost -addext subjectAltName=DNS:localhost,IP:127.0.0.1 >/work/test-cert/openssl.log 2>&1
cp /work/test-cert/server.crt /usr/local/share/ca-certificates/alice-fixture.crt
update-ca-certificates >/work/test-cert/trust.log 2>&1
sed -i '/^!include auth-system.conf.ext/s/^/#/' /etc/dovecot/conf.d/10-auth.conf
cat > /etc/dovecot/conf.d/99-alice-fixture.conf <<'CFG'
protocols = imap
listen = 127.0.0.1
mail_location = maildir:~/Maildir
ssl = required
ssl_cert = </work/test-cert/server.crt
ssl_key = </work/test-cert/server.key
auth_mechanisms = plain login
!include auth-passwdfile.conf.ext
service auth {
  unix_listener /var/spool/postfix/private/auth {
    mode = 0660
    user = postfix
    group = postfix
  }
}
namespace inbox {
  inbox = yes
  mailbox Gesendet {
    special_use = \Sent
    auto = create
  }
  mailbox Archiv {
    special_use = \Archive
    auto = create
  }
  mailbox Papierkorb {
    special_use = \Trash
    auto = create
  }
  mailbox "Alle Nachrichten" {
    special_use = \All
    auto = create
  }
}
CFG
sed -i 's|args = scheme=CRYPT username_format=%u /etc/dovecot/users|args = scheme=PLAIN username_format=%u /etc/dovecot/users|' /etc/dovecot/conf.d/auth-passwdfile.conf.ext
printf 'alice@example.test:{PLAIN}alice-fixture-password:1500:1500::/home/alice::\n' > /etc/dovecot/users
chown root:dovecot /etc/dovecot/users
chmod 640 /etc/dovecot/users
postconf -e 'myhostname=localhost' 'mydestination=localhost,example.test' 'inet_interfaces=127.0.0.1' 'inet_protocols=ipv4' 'home_mailbox=Maildir/' 'smtpd_tls_cert_file=/work/test-cert/server.crt' 'smtpd_tls_key_file=/work/test-cert/server.key' 'smtpd_sasl_type=dovecot' 'smtpd_sasl_path=private/auth' 'smtpd_sasl_auth_enable=yes' 'smtpd_relay_restrictions=permit_sasl_authenticated,reject' 'default_transport=error:fixture has no external delivery' 'relay_transport=error:fixture has no external delivery'
cat >> /etc/postfix/master.cf <<'CFG'
587 inet n - n - - smtpd
  -o smtpd_tls_security_level=encrypt
  -o smtpd_tls_auth_only=yes
465 inet n - n - - smtpd
  -o smtpd_tls_wrappermode=yes
  -o smtpd_tls_auth_only=yes
CFG
dovecot
postfix start
printf 'mail fixtures ready\n'
