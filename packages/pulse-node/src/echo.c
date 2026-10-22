#include <node_api.h>

static napi_value Echo(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  void *data = NULL;
  size_t length = 0;
  napi_value copy;
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  if (napi_get_buffer_info(env, argv[0], &data, &length) != napi_ok) {
    return NULL;
  }
  if (napi_create_buffer_copy(env, length, data, NULL, &copy) != napi_ok) {
    return NULL;
  }
  return copy;
}

NAPI_MODULE_INIT() {
  napi_value fn;
  napi_create_function(env, "echo", NAPI_AUTO_LENGTH, Echo, NULL, &fn);
  napi_set_named_property(env, exports, "echo", fn);
  return exports;
}
