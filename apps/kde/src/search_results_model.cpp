#include "search_results_model.h"
#include <QtQml/qqml.h>
extern "C" void register_search_results_model() {
    qmlRegisterType<SearchResultsModel>("org.nativeworkspace.Editor", 1, 0, "SearchResultsModel");
}
