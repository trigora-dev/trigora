FROM node:22-bookworm-slim
WORKDIR /opt/trigora
COPY node_modules /opt/trigora/node_modules
COPY helper/node-helper.js helper/pulse-adapter.mjs helper/ts-adapter.js /opt/trigora/helper/
COPY trigora trigora-local /usr/local/bin/
COPY app/ /opt/app/
RUN mkdir -p /opt/app/node_modules/@trigora/sdk \
  && printf '%s\n' '{"name":"@trigora/sdk","type":"module","exports":{".":"./index.js"}}' > /opt/app/node_modules/@trigora/sdk/package.json \
  && printf '%s\n' 'export function effect() {}' 'export function waitForEvent() {}' > /opt/app/node_modules/@trigora/sdk/index.js \
  && chmod -R a+rX /opt/trigora \
  && chmod -R a+rwX /opt/app
WORKDIR /opt/app
ENV TRIGORA_NODE_HELPER=/opt/trigora/helper/node-helper.js
ENV TRIGORA_PULSE_ADAPTER=/opt/trigora/helper/pulse-adapter.mjs
CMD ["node", "entrypoint.mjs"]
