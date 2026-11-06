import Foundation

/// Foundation-generated alerts/errors share the interface language selected in SwiftUI.
enum L10n {
    private static let lock = NSLock()
    private static var language = "system"
    private static var product = Product.shengJian
    static func use(_ value: String) { lock.withLock { language = value } }
    static func use(product value: Product) { lock.withLock { product = value } }
    /// The running app; shared views use it instead of naming one app.
    static var currentProduct: Product { lock.withLock { product } }
    /// Localizes a key whose `%@` stands for the app's name.
    static func named(_ key: String) -> String { String(format: text(key), currentProduct.name) }
    static func text(_ key: String) -> String {
        let selected = lock.withLock { language }
        let bundle: Bundle
        if selected != "system", let path = Bundle.main.path(forResource: selected, ofType: "lproj"), let localized = Bundle(path: path) { bundle = localized }
        else { bundle = .main }
        return bundle.localizedString(forKey: key, value: key, table: nil)
    }
}

extension SessionModel {
    public func localized(_ key: String) -> String { L10n.text(key) }
}
