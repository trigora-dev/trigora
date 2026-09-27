#!/usr/bin/env bash
# Pack authoring artifacts into release-candidate/. Does not publish.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
if [ -f "${root}/tcc-engine/frontends/typescript/package.json" ]; then
  engine="${root}/tcc-engine"
else
  engine="$(cd "${root}/../tcc-engine" && pwd)"
fi
ts="$(cd "${root}/../trigora-typescript" && pwd)"
py="$(cd "${root}/../trigora-python" && pwd)"
out="${root}/release-candidate"
mkdir -p "${out}/npm" "${out}/python" "${root}/dist-packages"

(
  cd "$engine"
  CI=true pnpm install --frozen-lockfile
  pnpm --filter @tcc-engine/frontend-typescript exec npm pack --pack-destination "${root}/dist-packages"
)
cp "${root}/dist-packages/tcc-engine-frontend-typescript-0.1.0.tgz" "${out}/npm/"

(
  cd "${root}/packages/contracts"
  npm install
  npm run build
  npm pack --pack-destination "${out}/npm"
)

(
  cd "$ts"
  CI=true pnpm install --frozen-lockfile
  pnpm --filter @trigora/sdk build
  pnpm --filter @trigora/sdk exec npm pack --pack-destination "${out}/npm"
  pnpm --filter @trigora/client build
  pnpm --filter @trigora/client exec npm pack --pack-destination "${out}/npm"
)

python3 -m pip install build maturin
python3 -m build --wheel --outdir "${out}/python" "${py}/trigora"
python3 -m build --wheel --outdir "${out}/python" "${py}/trigora-client"
python3 "${root}/python/trigora-cli/scripts/vendor_cli.py"
rm -rf "${root}/python/trigora-cli/build" "${root}/python/trigora-cli/src/trigora_cli.egg-info"
python3 -m build --wheel --outdir "${out}/python" "${root}/python/trigora-cli"
python3 -m build --wheel --outdir "${out}/python" "${engine}/bindings/python"

for tarball in "${out}/npm/trigora-sdk-0.9.0.tgz" "${out}/npm/trigora-client-0.9.0.tgz"; do
  if tar -xOf "$tarball" package/package.json | grep -q 'link:'; then
    echo "packed package.json still contains a link: dependency: ${tarball}" >&2
    exit 1
  fi
done

echo "authoring artifacts are in ${out}"
