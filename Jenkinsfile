pipeline {
  agent any

  options {
    disableConcurrentBuilds()
    timestamps()
  }

  parameters {
    choice(name: 'ACTION', choices: ['verify', 'deploy', 'rollback'], description: 'Pipeline action')
    choice(name: 'TARGET_ENVIRONMENT', choices: ['staging', 'production'], description: 'Deployment target')
  }

  stages {
    stage('Quality gate') {
      steps {
        sh '''#!/usr/bin/env bash
          set -euo pipefail
          rustup component add rustfmt clippy
          cargo fmt --all --check
          cargo build --locked
          cargo test --locked
          cargo clippy --all-features --locked -- -D warnings
          cargo test --test cli_smoke --locked
          git diff --exit-code Cargo.lock
        '''
      }
    }

    stage('Deploy or rollback') {
      when { expression { params.ACTION != 'verify' } }
      steps {
        script {
          if (params.TARGET_ENVIRONMENT == 'production') {
            input message: 'Approve production ' + params.ACTION + '?', ok: 'Approve'
          }
        }
        withCredentials([
          string(credentialsId: 'starforge-deploy-command', variable: 'STARFORGE_DEPLOY_COMMAND'),
          string(credentialsId: 'starforge-rollback-command', variable: 'STARFORGE_ROLLBACK_COMMAND'),
          string(credentialsId: 'starforge-healthcheck-url', variable: 'STARFORGE_HEALTHCHECK_URL')
        ]) {
          sh '''#!/usr/bin/env bash
            set -euo pipefail
            export STARFORGE_DEPLOY_ENVIRONMENT="$TARGET_ENVIRONMENT"
            if [ "$TARGET_ENVIRONMENT" = production ]; then
              export STARFORGE_DEPLOY_APPROVED=true STARFORGE_ROLLBACK_APPROVED=true
            fi
            if [ "$ACTION" = deploy ]; then
              bash scripts/ci-deploy.sh
            else
              bash scripts/ci-rollback.sh
            fi
          '''
        }
      }
    }
  }
}
